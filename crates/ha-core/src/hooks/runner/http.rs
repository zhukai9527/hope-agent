//! `http` hook handler — POSTs the hook input JSON to a URL and treats the
//! JSON response body as the hook's output (design §7.3).
//!
//! The outbound URL is SSRF-gated through `security::ssrf::check_url` (the
//! shared policy + trusted-host allowlist) before any network touch — new
//! outbound entries must never self-validate IPs (AGENTS.md red line). The
//! HTTP status is mapped to a synthetic exit code so the existing output
//! parser handles the body unchanged: 2xx → exit 0 (parse JSON body), any
//! other status → exit 1 (non-blocking).

use std::time::{Duration, Instant};

use async_trait::async_trait;

use super::super::config::HttpHookConfig;
use super::super::env::HookEnv;
use super::super::types::HookInput;
use super::{HookHandler, RawHookResult};

/// Default http-hook timeout (design §7.3).
const DEFAULT_HTTP_TIMEOUT_SECS: u64 = 30;
/// Response body capture cap (§7.9).
const MAX_RESPONSE_BYTES: usize = 1024 * 1024; // 1 MiB

pub struct HttpHandler {
    config: HttpHookConfig,
}

impl HttpHandler {
    pub fn new(config: HttpHookConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl HookHandler for HttpHandler {
    fn identity(&self) -> String {
        format!("{}|timeout={:?}", self.config.url, self.config.timeout)
    }

    fn handler_type(&self) -> &'static str {
        "http"
    }

    fn default_timeout(&self) -> Duration {
        Duration::from_secs(self.config.timeout.unwrap_or(DEFAULT_HTTP_TIMEOUT_SECS))
    }

    async fn run(&self, input: &HookInput, env: &HookEnv, deadline: Instant) -> RawHookResult {
        let start = Instant::now();

        // SSRF gate FIRST — before constructing the client or touching the
        // network. Uses the shared `Default` policy + the app's trusted-host
        // allowlist, identical to every other outbound dial-out site.
        let trusted = crate::config::cached_config().ssrf.trusted_hosts.clone();
        if let Err(e) = crate::security::ssrf::check_url(
            &self.config.url,
            crate::security::ssrf::SsrfPolicy::Default,
            &trusted,
        )
        .await
        {
            return RawHookResult::non_blocking_error(format!("hook http SSRF blocked: {e}"));
        }

        let body = match serde_json::to_vec(input) {
            Ok(b) => b,
            Err(e) => {
                return RawHookResult::non_blocking_error(format!("serialize hook input: {e}"))
            }
        };

        let timeout = deadline.saturating_duration_since(Instant::now());
        let builder = reqwest::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::limited(5));
        // Honor the app proxy policy (matches every other outbound site).
        let client = match crate::provider::apply_proxy(builder).build() {
            Ok(c) => c,
            Err(e) => {
                return RawHookResult::non_blocking_error(format!("build http client: {e}"))
            }
        };

        let mut req = client
            .post(&self.config.url)
            .header("content-type", "application/json")
            .body(body);
        // Static configured headers.
        for (k, v) in &self.config.headers {
            req = req.header(k, v);
        }
        // Forward whitelisted env vars as `X-Hope-Env-<NAME>` headers so the
        // endpoint can read the same context a command hook gets on its env,
        // without leaking the full set.
        for key in &self.config.allowed_env_vars {
            if let Some(val) = env.as_vars().get(key) {
                req = req.header(format!("X-Hope-Env-{key}"), val);
            }
        }

        let resp = match tokio::time::timeout(timeout, req.send()).await {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                return RawHookResult::non_blocking_error(format!("hook http error: {e}"))
            }
            Err(_) => {
                return RawHookResult {
                    exit_code: None,
                    stdout: String::new(),
                    stderr: format!("hook http timed out after {}s", timeout.as_secs()),
                    duration: start.elapsed(),
                    timed_out: true,
                }
            }
        };

        let status = resp.status();
        let text = match resp.text().await {
            Ok(t) => crate::truncate_utf8(&t, MAX_RESPONSE_BYTES).to_string(),
            Err(e) => {
                return RawHookResult::non_blocking_error(format!("read hook http body: {e}"))
            }
        };

        // Map the HTTP status to a synthetic exit code so the shared parser
        // handles the body: 2xx → exit 0 (parse JSON), else → exit 1 (inert).
        RawHookResult {
            exit_code: Some(if status.is_success() { 0 } else { 1 }),
            stdout: text,
            stderr: if status.is_success() {
                String::new()
            } else {
                format!("http status {}", status.as_u16())
            },
            duration: start.elapsed(),
            timed_out: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::types::{CommonHookInput, PermissionMode};
    use std::path::PathBuf;

    fn dummy_input() -> HookInput {
        HookInput::PreToolUse {
            common: CommonHookInput {
                session_id: "s1".into(),
                transcript_path: PathBuf::from("/tmp/t.jsonl"),
                cwd: PathBuf::from("/tmp"),
                permission_mode: PermissionMode::Default,
                hook_event_name: "PreToolUse".into(),
                agent_id: None,
                agent_type: None,
            },
            tool_name: "exec".into(),
            tool_input: serde_json::json!({}),
            tool_use_id: "c1".into(),
        }
    }

    /// A private-IP target is rejected by the SSRF gate before any network
    /// touch (literal IP → classified directly, no DNS), and surfaces as a
    /// non-blocking error rather than dialing out.
    #[tokio::test]
    async fn ssrf_blocks_private_target() {
        let h = HttpHandler::new(HttpHookConfig {
            url: "http://10.0.0.1/hook".into(),
            timeout: Some(5),
            headers: Default::default(),
            allowed_env_vars: vec![],
            status_message: None,
            if_rule: None,
            once: None,
        });
        let r = h
            .run(
                &dummy_input(),
                &HookEnv::empty(),
                Instant::now() + Duration::from_secs(5),
            )
            .await;
        assert_eq!(r.exit_code, Some(1));
        assert!(
            r.stderr.contains("SSRF"),
            "expected SSRF block, got {:?}",
            r.stderr
        );
        assert!(!r.timed_out);
    }
}
