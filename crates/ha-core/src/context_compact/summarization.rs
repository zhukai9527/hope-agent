// ── Tier 3: Summarization Helpers (used by agent.rs) ──

use super::config::CompactConfig;
use super::estimation::{estimate_tokens, get_tool_result_text, is_tool_result, is_user_message};
use super::task_notification::{
    build_summary_with_async_job_references, collect_async_job_references_from_message,
    collect_async_job_references_from_messages, render_async_job_reference_section,
    text_without_async_job_references,
};
use super::types::SummarizationSplit;
use super::{
    BASE_CHUNK_RATIO, IDENTIFIER_PRESERVATION_INSTRUCTIONS, MIN_CHUNK_RATIO, SAFETY_MARGIN,
};
use serde_json::Value;

/// System prompt for context summarization (Tier 3)
#[allow(dead_code)]
pub(crate) const SUMMARIZATION_SYSTEM_PROMPT: &str = r#"You are a context compaction assistant.
Summarize the conversation history into a concise summary that preserves all critical context.

MUST PRESERVE:
- Active tasks and their current status (in-progress, blocked, pending)
- Batch operation progress (e.g., "5/17 items completed")
- The last thing the user requested and what was being done about it
- Decisions made and their rationale
- TODOs, open questions, and constraints
- Any commitments or follow-ups promised
- All file paths, function names, and code references mentioned
- Async job notification references, preserving task-id, tool-use-id, tool, status, output-file, error, and summary exactly

PRIORITIZE recent context over older history. The agent needs to know what it was doing, not just what was discussed.

Output format:
## Decisions
## Open TODOs
## Constraints/Rules
## Pending user asks
## Exact identifiers
## Conversation summary
"#;

/// Split messages into summarizable (old) and preserved (recent) portions.
pub fn split_for_summarization(
    messages: &[Value],
    config: &CompactConfig,
) -> Option<SummarizationSplit> {
    let preserve = config.preserve_recent_turns.min(12).max(1);
    let mut user_count = 0;
    let mut boundary_index = 0;

    // Find the Nth-from-last user message as boundary
    for (i, msg) in messages.iter().enumerate().rev() {
        if is_user_message(msg) {
            user_count += 1;
            if user_count >= preserve {
                boundary_index = i;
                break;
            }
        }
    }

    if boundary_index == 0 || user_count < preserve {
        return None; // Not enough turns to summarize
    }

    // Adjust to a round-safe boundary so we never split a tool_use/tool_result pair
    boundary_index = super::round_grouping::find_round_safe_boundary(messages, boundary_index);

    if boundary_index == 0 {
        return None; // Round adjustment consumed all summarizable messages
    }

    let summarizable = messages[..boundary_index].to_vec();
    let preserved = messages[boundary_index..].to_vec();

    if summarizable.is_empty() {
        return None;
    }

    Some(SummarizationSplit {
        summarizable,
        preserved,
        preserved_start_index: boundary_index,
    })
}

/// Build a summarization prompt from messages to summarize.
pub fn build_summarization_prompt(
    messages_to_summarize: &[Value],
    previous_summary: Option<&str>,
    config: &CompactConfig,
) -> String {
    let mut prompt = String::new();

    // Add previous summary if exists
    if let Some(prev) = previous_summary {
        prompt.push_str("Previous conversation summary:\n");
        prompt.push_str(prev);
        prompt.push_str("\n\n---\n\n");
    }

    prompt.push_str("Conversation to summarize:\n\n");

    // Serialize messages in a readable format
    for msg in messages_to_summarize {
        let msg_type = msg.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let async_refs = collect_async_job_references_from_message(msg);
        let has_async_refs = !async_refs.is_empty();
        if has_async_refs {
            prompt.push_str("[async_job_reference]:");
            prompt.push_str(&render_async_job_reference_section(&async_refs));
            prompt.push('\n');
        }

        // Skip encrypted reasoning items (not human-readable)
        if msg_type == "reasoning" {
            continue;
        }

        // Responses API function_call → readable tool call
        if msg_type == "function_call" {
            let name = msg
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("unknown");
            let args = msg
                .get("arguments")
                .and_then(|a| a.as_str())
                .unwrap_or("{}");
            let args_preview = if args.len() > 200 {
                format!("{}...", crate::truncate_utf8(args, 200))
            } else {
                args.to_string()
            };
            prompt.push_str(&format!("[tool_call]: {}({})\n", name, args_preview));
            continue;
        }

        // Responses API function_call_output → readable tool result
        if msg_type == "function_call_output" {
            let output = msg.get("output").and_then(|o| o.as_str()).unwrap_or("");
            let output = prompt_text_without_async_refs(output, has_async_refs);
            if output.trim().is_empty() {
                continue;
            }
            let preview = if output.len() > 500 {
                format!(
                    "{}... [{}+ chars]",
                    crate::truncate_utf8(&output, 500),
                    output.len()
                )
            } else {
                output
            };
            prompt.push_str(&format!("[tool_result]: {}\n", preview));
            continue;
        }

        let role = msg
            .get("role")
            .and_then(|r| r.as_str())
            .unwrap_or("unknown");

        if is_tool_result(msg) {
            if let Some(text) = get_tool_result_text(msg) {
                let text = prompt_text_without_async_refs(&text, has_async_refs);
                if text.trim().is_empty() {
                    continue;
                }
                let preview = if text.len() > 500 {
                    format!(
                        "{}... [{}+ chars]",
                        crate::truncate_utf8(&text, 500),
                        text.len()
                    )
                } else {
                    text
                };
                prompt.push_str(&format!("[tool_result]: {}\n", preview));
            }
        } else if msg_type == "message" {
            // OpenAI Responses API message format
            if let Some(parts) = msg.get("content").and_then(|c| c.as_array()) {
                for part in parts {
                    if part.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                            push_role_text(&mut prompt, role, text, has_async_refs);
                        }
                    }
                }
            }
        } else if let Some(content) = msg.get("content").and_then(|c| c.as_str()) {
            // Simple string content (Chat Completions / Anthropic simple format)
            push_role_text(&mut prompt, role, content, has_async_refs);
        } else if let Some(content_arr) = msg.get("content").and_then(|c| c.as_array()) {
            // Array content (Anthropic format with thinking + text blocks)
            for block in content_arr {
                let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                match block_type {
                    "text" => {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            push_role_text(&mut prompt, role, text, has_async_refs);
                        }
                    }
                    "thinking" => {
                        if let Some(thinking) = block.get("thinking").and_then(|t| t.as_str()) {
                            let preview = if thinking.len() > 300 {
                                format!("{}...", crate::truncate_utf8(thinking, 300))
                            } else {
                                thinking.to_string()
                            };
                            prompt.push_str(&format!("[{}/thinking]: {}\n", role, preview));
                        }
                    }
                    _ => {}
                }
            }
        }

        // Chat Completions reasoning_content field
        if let Some(reasoning) = msg.get("reasoning_content").and_then(|r| r.as_str()) {
            if !reasoning.is_empty() {
                let preview = if reasoning.len() > 300 {
                    format!("{}...", crate::truncate_utf8(reasoning, 300))
                } else {
                    reasoning.to_string()
                };
                prompt.push_str(&format!("[{}/thinking]: {}\n", role, preview));
            }
        }
    }

    // Add identifier preservation instructions
    if config.identifier_policy != "off" {
        let instructions = if config.identifier_policy == "custom" {
            config
                .identifier_instructions
                .as_deref()
                .unwrap_or(IDENTIFIER_PRESERVATION_INSTRUCTIONS)
        } else {
            IDENTIFIER_PRESERVATION_INSTRUCTIONS
        };
        prompt.push_str("\n\nAdditional instructions:\n");
        prompt.push_str(instructions);
    }

    // Add custom instructions
    if let Some(ref custom) = config.custom_instructions {
        prompt.push_str("\n\n");
        prompt.push_str(custom);
    }

    prompt
}

fn push_role_text(prompt: &mut String, role: &str, text: &str, strip_async_refs: bool) {
    let text = prompt_text_without_async_refs(text, strip_async_refs);
    if !text.trim().is_empty() {
        prompt.push_str(&format!("[{}]: {}\n", role, text));
    }
}

fn prompt_text_without_async_refs(text: &str, strip_async_refs: bool) -> String {
    if strip_async_refs {
        text_without_async_job_references(text).trim().to_string()
    } else {
        text.to_string()
    }
}

/// Apply a summary: replace old messages with a summary message + preserved messages.
pub fn apply_summary(
    messages: &mut Vec<Value>,
    summary: &str,
    preserved_start_index: usize,
    config: &CompactConfig,
) {
    // Cap summary length (configurable, clamped to 4000–64000)
    let max_summary_chars = config.max_compaction_summary_chars.clamp(4_000, 64_000);
    let async_refs = if preserved_start_index <= messages.len() {
        collect_async_job_references_from_messages(&messages[..preserved_start_index])
    } else {
        Vec::new()
    };
    let capped_summary =
        build_summary_with_async_job_references(summary, &async_refs, max_summary_chars);

    // Build summary message
    let summary_msg = serde_json::json!({
        "role": "user",
        "content": format!("[Previous conversation summary]\n\n{}", capped_summary)
    });

    // Keep preserved messages
    let preserved: Vec<Value> = if preserved_start_index < messages.len() {
        messages[preserved_start_index..].to_vec()
    } else {
        Vec::new()
    };

    // Replace messages
    messages.clear();
    messages.push(summary_msg);
    messages.extend(preserved);
}

/// Check if a single message is too large to safely include in a summarization call.
#[allow(dead_code)]
pub fn is_oversized_for_summary(msg: &Value, context_window: u32) -> bool {
    let tokens = estimate_tokens(msg) as f64 * SAFETY_MARGIN;
    tokens > context_window as f64 * 0.5
}

/// Compute adaptive chunk ratio based on average message size.
#[allow(dead_code)]
pub fn compute_adaptive_chunk_ratio(messages: &[Value], context_window: u32) -> f64 {
    if messages.is_empty() || context_window == 0 {
        return BASE_CHUNK_RATIO;
    }

    let total_tokens: u32 = messages.iter().map(|m| estimate_tokens(m)).sum();
    let avg_tokens = total_tokens as f64 / messages.len() as f64;
    let safe_avg = avg_tokens * SAFETY_MARGIN;
    let avg_ratio = safe_avg / context_window as f64;

    if avg_ratio > 0.1 {
        let reduction = (avg_ratio * 2.0).min(BASE_CHUNK_RATIO - MIN_CHUNK_RATIO);
        (BASE_CHUNK_RATIO - reduction).max(MIN_CHUNK_RATIO)
    } else {
        BASE_CHUNK_RATIO
    }
}

/// Split messages into chunks by token share.
#[allow(dead_code)]
pub fn split_messages_by_token_share(messages: &[Value], parts: usize) -> Vec<Vec<Value>> {
    if messages.is_empty() {
        return vec![];
    }
    let parts = parts.max(1).min(messages.len());
    if parts <= 1 {
        return vec![messages.to_vec()];
    }

    let total_tokens: u32 = messages.iter().map(|m| estimate_tokens(m)).sum();
    let target_tokens = total_tokens / parts as u32;
    let mut chunks: Vec<Vec<Value>> = Vec::new();
    let mut current: Vec<Value> = Vec::new();
    let mut current_tokens: u32 = 0;

    for msg in messages {
        let msg_tokens = estimate_tokens(msg);
        if chunks.len() < parts - 1
            && !current.is_empty()
            && current_tokens + msg_tokens > target_tokens
        {
            chunks.push(current);
            current = Vec::new();
            current_tokens = 0;
        }
        current.push(msg.clone());
        current_tokens += msg_tokens;
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn summarization_prompt_canonicalizes_task_notifications() {
        let config = CompactConfig::default();
        let messages = vec![json!({
            "role": "user",
            "content": "<task-notification>\n<task-id>job_123</task-id>\n<tool-use-id>call_123</tool-use-id>\n<tool>exec</tool>\n<status>completed</status>\n<output-file>/tmp/output.txt</output-file>\n<output-preview>large content that should not be copied</output-preview>\n<summary>done</summary>\n</task-notification>"
        })];

        let prompt = build_summarization_prompt(&messages, None, &config);
        assert!(prompt.contains("[async_job_reference]:"));
        assert!(prompt.contains("<task-id>job_123</task-id>"));
        assert!(prompt.contains("<output-file>/tmp/output.txt</output-file>"));
        assert!(!prompt.contains("large content that should not be copied"));
    }

    #[test]
    fn summarization_prompt_preserves_summary_body_with_async_references() {
        let config = CompactConfig::default();
        let messages = vec![json!({
            "role": "user",
            "content": "[Previous conversation summary]\n\n## Decisions\n- Store async outputs on disk.\n\n## Async job references\n<async-job-reference>\n<task-id>job_123</task-id>\n<tool>exec</tool>\n<status>completed</status>\n<output-file>/tmp/output.txt</output-file>\n</async-job-reference>"
        })];

        let prompt = build_summarization_prompt(&messages, None, &config);
        assert!(prompt.contains("[async_job_reference]:"));
        assert!(prompt.contains("<task-id>job_123</task-id>"));
        assert!(prompt.contains("[user]: [Previous conversation summary]"));
        assert!(prompt.contains("- Store async outputs on disk."));
    }

    #[test]
    fn apply_summary_preserves_async_job_reference_when_model_omits_it() {
        let config = CompactConfig::default();
        let mut messages = vec![
            json!({
                "role": "user",
                "content": "<task-notification>\n<task-id>job_keep</task-id>\n<tool>exec</tool>\n<status>completed</status>\n<output-file>/tmp/result.txt</output-file>\n<summary>done</summary>\n</task-notification>"
            }),
            json!({"role": "user", "content": "recent"}),
        ];

        apply_summary(&mut messages, "Old discussion.", 1, &config);

        let summary = messages[0]["content"].as_str().unwrap();
        assert!(summary.contains("Old discussion."));
        assert!(summary.contains("<async-job-reference>"));
        assert!(summary.contains("<task-id>job_keep</task-id>"));
        assert!(summary.contains("<output-file>/tmp/result.txt</output-file>"));
        assert_eq!(messages[1]["content"].as_str(), Some("recent"));
    }
}
