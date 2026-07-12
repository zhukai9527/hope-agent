//! Provider-aware, content-free request token diagnostics.
//!
//! The manifest stores only byte counts, estimates, and short BLAKE3
//! fingerprints. It is safe to persist in logs without copying prompts,
//! memories, tool arguments, or conversation text.

use serde::Serialize;

use super::streaming_adapter::RoundRequest;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RoundTokenManifest {
    provider: &'static str,
    model: String,
    round: u32,
    request_shape: &'static str,
    stable_prompt_bytes: usize,
    stable_prompt_tokens_estimate: u32,
    stable_prompt_fingerprint: String,
    dynamic_prompt_bytes: usize,
    dynamic_prompt_tokens_estimate: u32,
    dynamic_prompt_fingerprint: String,
    history_bytes: usize,
    history_tokens_estimate: u32,
    history_fingerprint: String,
    tool_schema_bytes: usize,
    tool_schema_tokens_estimate: u32,
    tool_schema_fingerprint: String,
    eager_tool_schema_bytes: usize,
    eager_tool_schema_tokens_estimate: u32,
    eager_tool_schema_fingerprint: String,
    activated_tool_schema_bytes: usize,
    activated_tool_schema_tokens_estimate: u32,
    activated_tool_schema_fingerprint: String,
    deferred_tool_schema_bytes: usize,
    deferred_tool_schema_tokens_estimate: u32,
    deferred_tool_schema_fingerprint: String,
    eager_tool_count: usize,
    deferred_tool_count: usize,
    activated_tool_count: usize,
    cacheable_stable_tokens_estimate: u32,
    request_input_tokens_estimate: u32,
    transport_body_bytes: usize,
    native_deferred: bool,
}

fn token_estimate_bytes(bytes: usize) -> u32 {
    (bytes / crate::context_compact::CHARS_PER_TOKEN) as u32
}

fn fingerprint(value: &[u8]) -> String {
    blake3::hash(value).to_hex()[..16].to_string()
}

fn json_bytes(values: &[serde_json::Value]) -> usize {
    values
        .iter()
        .map(|value| {
            serde_json::to_vec(value)
                .map(|v| v.len())
                .unwrap_or_default()
        })
        .sum()
}

fn json_fingerprint(values: &[serde_json::Value]) -> String {
    let mut hasher = blake3::Hasher::new();
    for value in values {
        if let Ok(encoded) = serde_json::to_vec(value) {
            hasher.update(&encoded);
        }
    }
    hasher.finalize().to_hex()[..16].to_string()
}

impl RoundTokenManifest {
    pub(crate) fn from_request(
        provider: &'static str,
        model: &str,
        request_shape: &'static str,
        req: &RoundRequest<'_>,
        transport_body_bytes: usize,
        native_deferred: bool,
    ) -> Self {
        let dynamic_parts = [
            req.awareness_suffix,
            req.active_memory_suffix,
            req.coding_profile_suffix,
            req.procedure_memory_suffix,
            req.related_notes_suffix,
            req.task_reminder_suffix,
        ];
        let mut dynamic_hasher = blake3::Hasher::new();
        let dynamic_prompt_bytes = dynamic_parts
            .iter()
            .flatten()
            .map(|value| {
                dynamic_hasher.update(value.as_bytes());
                value.len()
            })
            .sum();
        let history_bytes = json_bytes(req.history_for_api);
        let tool_schema_bytes = json_bytes(req.tool_schemas);
        let eager_end = req.eager_tool_count.min(req.tool_schemas.len());
        let eager_tools = &req.tool_schemas[..eager_end];
        let activated_tools = &req.tool_schemas[eager_end..];
        let eager_tool_schema_bytes = json_bytes(eager_tools);
        let activated_tool_schema_bytes = json_bytes(activated_tools);
        let deferred_tool_schema_bytes = json_bytes(req.deferred_tool_schemas);
        let stable_prompt_tokens_estimate = token_estimate_bytes(req.system_prompt.len());
        let tool_schema_tokens_estimate = req
            .tool_schemas
            .iter()
            .map(crate::context_compact::estimate_tokens)
            .sum();
        let eager_tool_schema_tokens_estimate = eager_tools
            .iter()
            .map(crate::context_compact::estimate_tokens)
            .sum();
        let activated_tool_schema_tokens_estimate = activated_tools
            .iter()
            .map(crate::context_compact::estimate_tokens)
            .sum();
        let deferred_tool_schema_tokens_estimate = req
            .deferred_tool_schemas
            .iter()
            .map(crate::context_compact::estimate_tokens)
            .sum();
        let dynamic_prompt_tokens_estimate = token_estimate_bytes(dynamic_prompt_bytes);
        let history_tokens_estimate = req
            .history_for_api
            .iter()
            .map(crate::context_compact::estimate_tokens)
            .sum();
        let cacheable_stable_tokens_estimate =
            stable_prompt_tokens_estimate.saturating_add(eager_tool_schema_tokens_estimate);
        let request_input_tokens_estimate = cacheable_stable_tokens_estimate
            .saturating_add(activated_tool_schema_tokens_estimate)
            .saturating_add(dynamic_prompt_tokens_estimate)
            .saturating_add(history_tokens_estimate);

        Self {
            provider,
            model: model.to_string(),
            round: req.round,
            request_shape,
            stable_prompt_bytes: req.system_prompt.len(),
            stable_prompt_tokens_estimate,
            stable_prompt_fingerprint: fingerprint(req.system_prompt.as_bytes()),
            dynamic_prompt_bytes,
            dynamic_prompt_tokens_estimate,
            dynamic_prompt_fingerprint: dynamic_hasher.finalize().to_hex()[..16].to_string(),
            history_bytes,
            history_tokens_estimate,
            history_fingerprint: json_fingerprint(req.history_for_api),
            tool_schema_bytes,
            tool_schema_tokens_estimate,
            tool_schema_fingerprint: json_fingerprint(req.tool_schemas),
            eager_tool_schema_bytes,
            eager_tool_schema_tokens_estimate,
            eager_tool_schema_fingerprint: json_fingerprint(eager_tools),
            activated_tool_schema_bytes,
            activated_tool_schema_tokens_estimate,
            activated_tool_schema_fingerprint: json_fingerprint(activated_tools),
            deferred_tool_schema_bytes,
            deferred_tool_schema_tokens_estimate,
            deferred_tool_schema_fingerprint: json_fingerprint(req.deferred_tool_schemas),
            eager_tool_count: req.eager_tool_count,
            deferred_tool_count: req.deferred_tool_count,
            activated_tool_count: req.activated_tool_count,
            cacheable_stable_tokens_estimate,
            request_input_tokens_estimate,
            transport_body_bytes,
            native_deferred,
        }
    }

    pub(crate) fn log(&self) {
        if let Some(logger) = crate::get_logger() {
            logger.log(
                "info",
                "agent",
                "agent::round_token_manifest",
                &format!(
                    "{} round {}: estimated {} input tokens ({} eager / {} deferred / {} activated)",
                    self.provider,
                    self.round,
                    self.request_input_tokens_estimate,
                    self.eager_tool_count,
                    self.deferred_tool_count,
                    self.activated_tool_count,
                ),
                serde_json::to_string(self).ok(),
                None,
                None,
            );
        }
    }
}

pub(crate) fn log_round_manifest(
    provider: &'static str,
    model: &str,
    request_shape: &'static str,
    req: &RoundRequest<'_>,
    transport_body_bytes: usize,
    native_deferred: bool,
) {
    RoundTokenManifest::from_request(
        provider,
        model,
        request_shape,
        req,
        transport_body_bytes,
        native_deferred,
    )
    .log();
}

/// Complete the request-side manifest with the provider's authoritative
/// usage counters. Kept as a separate correlated log row because streaming
/// adapters only know usage/TTFT after the request manifest has been emitted.
pub(crate) fn log_round_usage(
    provider: &'static str,
    model: &str,
    round: u32,
    usage: &super::types::ChatUsage,
    ttft_ms: Option<u64>,
) {
    if let Some(logger) = crate::get_logger() {
        let details = serde_json::json!({
            "provider": provider,
            "model": model,
            "round": round,
            "contextInputTokens": usage.context_input_tokens,
            "freshInputTokens": usage.fresh_input_tokens,
            "cacheReadTokens": usage.cache_read_input_tokens,
            "cacheWriteTokens": usage.cache_creation_input_tokens,
            "outputTokens": usage.output_tokens,
            "ttftMs": ttft_ms,
        });
        logger.log(
            "info",
            "agent",
            "agent::round_token_usage",
            &format!(
                "{} round {}: actual {} context / {} fresh / {} cache-read tokens",
                provider,
                round,
                usage.context_input_tokens,
                usage.fresh_input_tokens,
                usage.cache_read_input_tokens,
            ),
            Some(details.to_string()),
            None,
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::RoundTokenManifest;
    use crate::agent::streaming_adapter::RoundRequest;

    #[test]
    fn legacy_empty_hi_baseline_is_locked() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/context/empty_hi_legacy_32738.json"
        ))
        .unwrap();
        assert_eq!(fixture["observedUsage"]["inputTokens"], 32_738);
        assert_eq!(fixture["historyMessagesBeforeTurn"], 0);
        assert_eq!(fixture["v2Acceptance"]["contextInputTokensMax"], 10_000);
        assert_eq!(fixture["v2Acceptance"]["eagerToolSchemaTokensMax"], 4_000);
        assert_eq!(fixture["v2Acceptance"]["staticPromptTokensMax"], 6_000);
    }

    #[test]
    fn dynamic_suffix_and_activation_do_not_change_stable_fingerprint() {
        let eager = vec![serde_json::json!({ "type": "function", "name": "read" })];
        let with_activation = vec![
            serde_json::json!({ "type": "function", "name": "read" }),
            serde_json::json!({ "type": "function", "name": "browser__snapshot" }),
        ];
        let deferred = Vec::new();
        let history = vec![serde_json::json!({ "role": "user", "content": "hi" })];
        let make = |tools: &[serde_json::Value], dynamic: &str| {
            let req = RoundRequest {
                system_prompt: "stable system",
                awareness_suffix: Some(dynamic),
                active_memory_suffix: None,
                coding_profile_suffix: None,
                procedure_memory_suffix: None,
                related_notes_suffix: None,
                task_reminder_suffix: None,
                tool_schemas: tools,
                deferred_tool_schemas: &deferred,
                eager_tool_count: 1,
                deferred_tool_count: 0,
                activated_tool_count: tools.len().saturating_sub(1),
                prompt_cache_key: Some("stable-key"),
                history_for_api: &history,
                reasoning_effort: None,
                temperature: None,
                max_tokens: 100,
                is_final_round: false,
                round: 0,
            };
            RoundTokenManifest::from_request("test", "model", "shape", &req, 0, false)
        };
        let first = make(&eager, "dynamic-a");
        let second = make(&with_activation, "dynamic-b");
        assert_eq!(
            first.stable_prompt_fingerprint,
            second.stable_prompt_fingerprint
        );
        assert_eq!(
            first.eager_tool_schema_fingerprint,
            second.eager_tool_schema_fingerprint
        );
        assert_eq!(
            first.cacheable_stable_tokens_estimate,
            second.cacheable_stable_tokens_estimate
        );
        assert_ne!(
            first.dynamic_prompt_fingerprint,
            second.dynamic_prompt_fingerprint
        );
        assert!(second.activated_tool_schema_tokens_estimate > 0);
    }
}
