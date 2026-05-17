use serde_json::Value;

use super::SUMMARY_TRUNCATED_MARKER;

const TASK_NOTIFICATION_TAG: &str = "task-notification";
const ASYNC_JOB_REFERENCE_TAG: &str = "async-job-reference";
const ASYNC_JOB_REFERENCE_HEADING: &str = "## Async job references";
const MAX_RENDERED_REFS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AsyncJobReference {
    task_id: String,
    tool_use_id: Option<String>,
    tool: Option<String>,
    status: Option<String>,
    output_file: Option<String>,
    media_items_json: Option<String>,
    output_preview: Option<String>,
    error: Option<String>,
    summary: Option<String>,
}

impl AsyncJobReference {
    fn merge_from(&mut self, newer: Self) {
        self.tool_use_id = prefer_non_empty(newer.tool_use_id, self.tool_use_id.take());
        self.tool = prefer_non_empty(newer.tool, self.tool.take());
        self.status = prefer_non_empty(newer.status, self.status.take());
        self.output_file = prefer_non_empty(newer.output_file, self.output_file.take());
        self.media_items_json =
            prefer_non_empty(newer.media_items_json, self.media_items_json.take());
        self.output_preview = prefer_non_empty(newer.output_preview, self.output_preview.take());
        self.error = prefer_non_empty(newer.error, self.error.take());
        self.summary = prefer_non_empty(newer.summary, self.summary.take());
    }
}

pub(crate) fn collect_async_job_references_from_messages(
    messages: &[Value],
) -> Vec<AsyncJobReference> {
    let mut refs = Vec::new();
    for msg in messages {
        merge_many(&mut refs, collect_async_job_references_from_message(msg));
    }
    refs
}

pub(crate) fn collect_async_job_references_from_message(msg: &Value) -> Vec<AsyncJobReference> {
    let mut refs = Vec::new();

    if let Some(content) = msg.get("content").and_then(|c| c.as_str()) {
        merge_many(&mut refs, collect_async_job_references_from_text(content));
    }

    if let Some(parts) = msg.get("content").and_then(|c| c.as_array()) {
        for part in parts {
            for key in ["text", "output_text", "content"] {
                if let Some(text) = part.get(key).and_then(|v| v.as_str()) {
                    merge_many(&mut refs, collect_async_job_references_from_text(text));
                }
            }
        }
    }

    refs
}

pub(crate) fn collect_async_job_references_from_text(text: &str) -> Vec<AsyncJobReference> {
    let mut refs = Vec::new();
    for block in extract_reference_blocks_in_order(text) {
        if let Some(reference) = parse_reference_block(block) {
            merge_reference(&mut refs, reference);
        }
    }
    refs
}

pub(crate) fn text_without_async_job_references(text: &str) -> String {
    let without_notifications = strip_tagged_blocks(text, TASK_NOTIFICATION_TAG);
    let without_refs = strip_tagged_blocks(&without_notifications, ASYNC_JOB_REFERENCE_TAG);
    without_refs
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed != ASYNC_JOB_REFERENCE_HEADING
                && !trimmed.starts_with("<omitted-older-async-job-references>")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn render_async_job_reference_section(refs: &[AsyncJobReference]) -> String {
    if refs.is_empty() {
        return String::new();
    }

    let total = refs.len();
    let start = total.saturating_sub(MAX_RENDERED_REFS);
    render_reference_section_from(&refs[start..], start)
}

fn render_async_job_reference_section_to_fit(
    refs: &[AsyncJobReference],
    max_chars: usize,
) -> String {
    if refs.is_empty() {
        return String::new();
    }

    let total = refs.len();
    let mut keep = total.min(MAX_RENDERED_REFS);
    loop {
        let start = total.saturating_sub(keep);
        let section = render_reference_section_from(&refs[start..], start);
        if section.len() <= max_chars || keep <= 1 {
            return section;
        }
        keep -= 1;
    }
}

fn render_reference_section_from(rendered: &[AsyncJobReference], omitted_older: usize) -> String {
    let mut out = String::new();
    out.push_str("\n\n");
    out.push_str(ASYNC_JOB_REFERENCE_HEADING);
    out.push('\n');
    if omitted_older > 0 {
        out.push_str(&format!(
            "<omitted-older-async-job-references>{}</omitted-older-async-job-references>\n",
            omitted_older
        ));
    }

    for reference in rendered {
        out.push_str("<async-job-reference>\n");
        push_required_tag(&mut out, "task-id", &reference.task_id, 256);
        push_optional_tag(
            &mut out,
            "tool-use-id",
            reference.tool_use_id.as_deref(),
            256,
        );
        push_optional_tag(&mut out, "tool", reference.tool.as_deref(), 128);
        push_optional_tag(&mut out, "status", reference.status.as_deref(), 64);
        push_optional_tag(
            &mut out,
            "output-file",
            reference.output_file.as_deref(),
            1024,
        );
        push_optional_tag(
            &mut out,
            "media-items-json",
            reference.media_items_json.as_deref(),
            512,
        );
        if reference.output_file.is_none() {
            push_optional_tag(
                &mut out,
                "output-preview",
                reference.output_preview.as_deref(),
                512,
            );
        }
        push_optional_tag(&mut out, "error", reference.error.as_deref(), 512);
        push_optional_tag(&mut out, "summary", reference.summary.as_deref(), 512);
        out.push_str("</async-job-reference>\n");
    }

    out
}

pub(crate) fn build_summary_with_async_job_references(
    summary: &str,
    source_refs: &[AsyncJobReference],
    max_chars: usize,
) -> String {
    let mut refs = collect_async_job_references_from_text(summary);
    merge_many(&mut refs, source_refs.to_vec());

    let section = render_async_job_reference_section_to_fit(&refs, max_chars);
    let body = if section.is_empty() {
        summary.to_string()
    } else {
        text_without_async_job_references(summary)
            .trim_end()
            .to_string()
    };

    cap_summary_preserving_suffix(&body, &section, max_chars)
}

fn parse_reference_block(block: &str) -> Option<AsyncJobReference> {
    let task_id = field(block, "task-id")?;
    if task_id.trim().is_empty() {
        return None;
    }

    Some(AsyncJobReference {
        task_id,
        tool_use_id: field(block, "tool-use-id"),
        tool: field(block, "tool"),
        status: field(block, "status"),
        output_file: field(block, "output-file"),
        media_items_json: field(block, "media-items-json"),
        output_preview: field(block, "output-preview"),
        error: field(block, "error"),
        summary: field(block, "summary"),
    })
}

fn field(block: &str, tag: &str) -> Option<String> {
    let raw = extract_first(block, tag)?;
    let value = unescape_xml_text(raw.trim()).trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn extract_blocks<'a>(text: &'a str, tag: &str) -> Vec<&'a str> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let mut out = Vec::new();
    let mut rest = text;

    while let Some(start) = rest.find(&open) {
        let after_open = &rest[start + open.len()..];
        let Some(end) = after_open.find(&close) else {
            break;
        };
        out.push(&after_open[..end]);
        rest = &after_open[end + close.len()..];
    }

    out
}

fn extract_reference_blocks_in_order(text: &str) -> Vec<&str> {
    let mut blocks = Vec::new();
    for tag in [TASK_NOTIFICATION_TAG, ASYNC_JOB_REFERENCE_TAG] {
        for (start, block) in extract_blocks_with_start(text, tag) {
            blocks.push((start, block));
        }
    }
    blocks.sort_by_key(|(start, _)| *start);
    blocks.into_iter().map(|(_, block)| block).collect()
}

fn extract_blocks_with_start<'a>(text: &'a str, tag: &str) -> Vec<(usize, &'a str)> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let mut out = Vec::new();
    let mut search_from = 0;

    while search_from < text.len() {
        let Some(relative_start) = text[search_from..].find(&open) else {
            break;
        };
        let start = search_from + relative_start;
        let body_start = start + open.len();
        let Some(relative_end) = text[body_start..].find(&close) else {
            break;
        };
        let body_end = body_start + relative_end;
        out.push((start, &text[body_start..body_end]));
        search_from = body_end + close.len();
    }

    out
}

fn extract_first<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    extract_blocks(text, tag).into_iter().next()
}

fn merge_many(refs: &mut Vec<AsyncJobReference>, incoming: Vec<AsyncJobReference>) {
    for reference in incoming {
        merge_reference(refs, reference);
    }
}

fn merge_reference(refs: &mut Vec<AsyncJobReference>, reference: AsyncJobReference) {
    if let Some(pos) = refs.iter().position(|r| r.task_id == reference.task_id) {
        let mut existing = refs.remove(pos);
        existing.merge_from(reference);
        refs.push(existing);
    } else {
        refs.push(reference);
    }
}

fn prefer_non_empty(newer: Option<String>, older: Option<String>) -> Option<String> {
    match newer {
        Some(value) if !value.trim().is_empty() => Some(value),
        _ => older,
    }
}

fn push_required_tag(out: &mut String, tag: &str, value: &str, max_chars: usize) {
    push_tag(out, tag, value, max_chars);
}

fn push_optional_tag(out: &mut String, tag: &str, value: Option<&str>, max_chars: usize) {
    if let Some(value) = value {
        if !value.trim().is_empty() {
            push_tag(out, tag, value, max_chars);
        }
    }
}

fn push_tag(out: &mut String, tag: &str, value: &str, max_chars: usize) {
    let value = limit_field(value, max_chars);
    out.push_str(&format!("<{}>{}</{}>\n", tag, escape_xml_text(&value), tag));
}

fn limit_field(value: &str, max_chars: usize) -> String {
    if value.len() <= max_chars {
        value.to_string()
    } else {
        format!("{}... [truncated]", crate::truncate_utf8(value, max_chars))
    }
}

fn strip_tagged_blocks(text: &str, tag: &str) -> String {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let mut out = String::new();
    let mut rest = text;

    while let Some(start) = rest.find(&open) {
        out.push_str(&rest[..start]);
        let after_open = &rest[start + open.len()..];
        let Some(end) = after_open.find(&close) else {
            out.push_str(rest);
            return out;
        };
        rest = &after_open[end + close.len()..];
    }
    out.push_str(rest);
    out
}

fn cap_summary_preserving_suffix(body: &str, suffix: &str, max_chars: usize) -> String {
    let max_chars = max_chars.max(1);
    if suffix.is_empty() {
        return cap_text(body, max_chars);
    }

    if suffix.len() >= max_chars {
        return cap_text(suffix, max_chars);
    }

    let available_for_body = max_chars - suffix.len();
    if body.len() <= available_for_body {
        return format!("{}{}", body, suffix);
    }

    let body_budget = available_for_body.saturating_sub(SUMMARY_TRUNCATED_MARKER.len());
    format!(
        "{}{}{}",
        crate::truncate_utf8(body, body_budget),
        SUMMARY_TRUNCATED_MARKER,
        suffix
    )
}

fn cap_text(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        text.to_string()
    } else {
        let budget = max_chars.saturating_sub(SUMMARY_TRUNCATED_MARKER.len());
        format!(
            "{}{}",
            crate::truncate_utf8(text, budget),
            SUMMARY_TRUNCATED_MARKER
        )
    }
}

fn escape_xml_text(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn unescape_xml_text(input: &str) -> String {
    input
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_task_notification_and_renders_reference() {
        let text = r#"<task-notification>
<task-id>job_123</task-id>
<tool-use-id>call_456</tool-use-id>
<tool>exec</tool>
<status>completed</status>
<output-file>/tmp/out&amp;1.txt</output-file>
<summary>done</summary>
</task-notification>"#;

        let refs = collect_async_job_references_from_text(text);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].task_id, "job_123");
        assert_eq!(refs[0].output_file.as_deref(), Some("/tmp/out&1.txt"));

        let rendered = render_async_job_reference_section(&refs);
        assert!(rendered.contains("<async-job-reference>"));
        assert!(rendered.contains("<task-id>job_123</task-id>"));
        assert!(rendered.contains("<output-file>/tmp/out&amp;1.txt</output-file>"));
    }

    #[test]
    fn later_references_update_existing_task_status() {
        let text = r#"<async-job-reference>
<task-id>job_1</task-id>
<status>running</status>
</async-job-reference>
<task-notification>
<task-id>job_1</task-id>
<status>completed</status>
<output-file>/tmp/result.txt</output-file>
</task-notification>"#;

        let refs = collect_async_job_references_from_text(text);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].status.as_deref(), Some("completed"));
        assert_eq!(refs[0].output_file.as_deref(), Some("/tmp/result.txt"));
    }

    #[test]
    fn updated_reference_is_treated_as_recent_when_rendering_limit_applies() {
        let mut text = String::new();
        text.push_str("<async-job-reference>\n<task-id>job_0</task-id>\n<status>running</status>\n</async-job-reference>\n");
        for i in 1..=32 {
            text.push_str(&format!(
                "<async-job-reference>\n<task-id>job_{}</task-id>\n<status>completed</status>\n</async-job-reference>\n",
                i
            ));
        }
        text.push_str("<task-notification>\n<task-id>job_0</task-id>\n<status>completed</status>\n<output-file>/tmp/job0.txt</output-file>\n</task-notification>\n");

        let refs = collect_async_job_references_from_text(&text);
        let rendered = render_async_job_reference_section(&refs);

        assert!(rendered.contains("<task-id>job_0</task-id>"));
        assert!(rendered.contains("<output-file>/tmp/job0.txt</output-file>"));
        assert!(!rendered.contains("<task-id>job_1</task-id>"));
    }

    #[test]
    fn strips_reference_blocks_without_dropping_surrounding_summary() {
        let text = "Decision: keep output on disk.\n\n## Async job references\n<async-job-reference>\n<task-id>job_1</task-id>\n</async-job-reference>\nNext step: inspect output.";

        let stripped = text_without_async_job_references(text);

        assert!(stripped.contains("Decision: keep output on disk."));
        assert!(stripped.contains("Next step: inspect output."));
        assert!(!stripped.contains("<async-job-reference>"));
        assert!(!stripped.contains("## Async job references"));
    }

    #[test]
    fn summary_cap_keeps_async_reference_suffix() {
        let refs = collect_async_job_references_from_text(
            r#"<task-notification>
<task-id>job_keep</task-id>
<tool>exec</tool>
<status>completed</status>
<output-file>/tmp/result.txt</output-file>
</task-notification>"#,
        );

        let summary = "x".repeat(5000);
        let compacted = build_summary_with_async_job_references(&summary, &refs, 4096);
        assert!(compacted.contains(SUMMARY_TRUNCATED_MARKER));
        assert!(compacted.contains("<task-id>job_keep</task-id>"));
        assert!(compacted.contains("<output-file>/tmp/result.txt</output-file>"));
    }
}
