//! R7.4 retry policy for backgrounded tool jobs.
//!
//! A backgrounded job that *fails* or *times out* may be retried with
//! exponential backoff — but **safely by default**: only side-effect-free,
//! network-transient tools are ever retried. The eligibility set is a
//! CODE-LEVEL safety decision (not a user knob), because a blind re-run of a
//! side-effecting tool (`exec`) could repeat a half-applied effect, and a
//! re-run of a cost-heavy non-deterministic tool (`image_generate`) re-bills
//! for a different result. User-initiated cancels and policy denials are never
//! retried.
//!
//! This module is the pure decision layer: [`decide`] takes the tool name, the
//! attempt number, the terminal [`JobError`], and the resolved [`RetryConfig`],
//! and returns whether to retry (and the backoff). The worker loop in
//! [`super::spawn`] drives it. Keeping the policy pure makes it unit-testable
//! without a runtime.

use super::error::JobError;

/// Fixed exponential-backoff base. Not a user knob — the user-facing tunables
/// are the master switch + attempt count; the timing curve is fixed so a typo
/// can't turn retries into a multi-minute stall.
const BASE_BACKOFF_MS: u64 = 500;

/// Cap on the backoff shift so `1 << n` can never overflow / explode for a
/// pathological `max_retry_attempts`.
const MAX_BACKOFF_SHIFT: u32 = 6; // 500ms * 2^6 = 32s ceiling

/// Resolved retry knobs (snapshot of `async_tools.{retry_enabled,max_retry_attempts}`).
#[derive(Debug, Clone, Copy)]
pub struct RetryConfig {
    pub enabled: bool,
    /// Total attempts (1 = no retry; the initial try counts).
    pub max_attempts: u32,
}

impl RetryConfig {
    /// Read the live config snapshot.
    pub fn current() -> Self {
        let cfg = crate::config::cached_config();
        Self {
            enabled: cfg.async_tools.retry_enabled,
            max_attempts: cfg.async_tools.max_retry_attempts,
        }
    }
}

/// What to do after an attempt failed.
#[derive(Debug, PartialEq, Eq)]
pub enum RetryDecision {
    /// Settle the job with the failure — no (more) retries.
    Stop,
    /// Retry after sleeping `backoff_ms` (cancellable).
    Retry { backoff_ms: u64 },
}

/// Tools whose failures are typically transient (network) AND whose
/// re-execution is side-effect-free, so a backgrounded job may auto-retry them.
///
/// Deliberately NOT here (never auto-retried, by design):
/// - `exec` — a shell command may have applied a partial side effect before
///   failing; a blind re-run could repeat it.
/// - `image_generate` — re-bills and returns a different (non-deterministic)
///   image; retrying is a cost decision the model/user should make explicitly.
///
/// Only async-capable tools ever reach the job path; today that's `web_search`
/// among the eligible set (`web_fetch` is listed for forward-compatibility but
/// is not async-capable yet, so it never becomes a background job).
pub fn is_retry_eligible(tool_name: &str) -> bool {
    matches!(
        tool_name,
        crate::tools::TOOL_WEB_SEARCH | crate::tools::TOOL_WEB_FETCH
    )
}

/// Decide whether to retry tool `tool_name` after its `attempt`-th run (1-based)
/// ended with `error`.
///
/// Rules:
/// - Retry only [`JobError::Failed`] (a transient tool error where the dispatch
///   returned cleanly — e.g. a network blip / rate-limit from `web_search`), and
///   only when retries are enabled, the tool is [`is_retry_eligible`], and
///   `attempt < max_attempts`. Backoff is exponential from [`BASE_BACKOFF_MS`].
/// - **Never** retry [`JobError::Cancelled`] (terminal user/session cancel),
///   [`JobError::DeniedByUser`] (deterministic — a re-run won't be approved), or
///   [`JobError::TimedOut`]. Timeout is deliberately excluded: the per-job
///   timeout cancels the *shared* job token (so the worker can't cleanly start a
///   fresh attempt without per-attempt token plumbing), and a tool that
///   exhausted its time budget is likely to time out again. Failed covers the
///   dominant transient case (API errors), which is the safe, useful scope.
pub fn decide(tool_name: &str, attempt: u32, error: &JobError, cfg: &RetryConfig) -> RetryDecision {
    match error {
        // The only retryable class: a clean dispatch error (token untouched).
        JobError::Failed { .. } => {}
        // Terminal regardless of policy (see fn doc for why TimedOut is here).
        JobError::Cancelled | JobError::DeniedByUser { .. } | JobError::TimedOut { .. } => {
            return RetryDecision::Stop
        }
    }
    if !cfg.enabled || cfg.max_attempts <= 1 || !is_retry_eligible(tool_name) {
        return RetryDecision::Stop;
    }
    if attempt >= cfg.max_attempts {
        return RetryDecision::Stop;
    }
    // attempt is 1-based; the 1st failure backs off 500ms, 2nd 1s, …
    let shift = (attempt - 1).min(MAX_BACKOFF_SHIFT);
    RetryDecision::Retry {
        backoff_ms: BASE_BACKOFF_MS.saturating_mul(1u64 << shift),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(enabled: bool, max: u32) -> RetryConfig {
        RetryConfig {
            enabled,
            max_attempts: max,
        }
    }
    fn failed() -> JobError {
        JobError::Failed {
            message: "boom".into(),
        }
    }

    #[test]
    fn eligible_tool_retries_until_attempts_exhausted() {
        let c = cfg(true, 3);
        assert_eq!(
            decide("web_search", 1, &failed(), &c),
            RetryDecision::Retry { backoff_ms: 500 }
        );
        assert_eq!(
            decide("web_search", 2, &failed(), &c),
            RetryDecision::Retry { backoff_ms: 1000 }
        );
        // 3rd attempt is the last (max_attempts=3) → stop.
        assert_eq!(decide("web_search", 3, &failed(), &c), RetryDecision::Stop);
    }

    #[test]
    fn side_effect_tools_never_retry() {
        let c = cfg(true, 3);
        assert_eq!(decide("exec", 1, &failed(), &c), RetryDecision::Stop);
        assert_eq!(
            decide("image_generate", 1, &failed(), &c),
            RetryDecision::Stop
        );
    }

    #[test]
    fn cancelled_and_denied_never_retry_even_for_eligible_tool() {
        let c = cfg(true, 5);
        assert_eq!(
            decide("web_search", 1, &JobError::Cancelled, &c),
            RetryDecision::Stop
        );
        let denied = JobError::DeniedByUser {
            rejection: crate::tools::rejection::ToolRejection::DeniedByUser {
                name: "web_search".into(),
            },
        };
        assert_eq!(decide("web_search", 1, &denied, &c), RetryDecision::Stop);
    }

    #[test]
    fn timeout_never_retries_even_for_eligible_tool() {
        // TimedOut is deliberately excluded (see decide() doc): the per-job
        // timeout cancels the shared job token, and a budget-exhausted tool
        // tends to time out again. Only Failed retries.
        let c = cfg(true, 3);
        assert_eq!(
            decide("web_search", 1, &JobError::TimedOut { max_secs: 30 }, &c),
            RetryDecision::Stop
        );
        assert_eq!(
            decide("exec", 1, &JobError::TimedOut { max_secs: 30 }, &c),
            RetryDecision::Stop
        );
    }

    #[test]
    fn disabled_or_single_attempt_never_retries() {
        assert_eq!(
            decide("web_search", 1, &failed(), &cfg(false, 3)),
            RetryDecision::Stop,
            "master switch off"
        );
        assert_eq!(
            decide("web_search", 1, &failed(), &cfg(true, 1)),
            RetryDecision::Stop,
            "max_attempts=1 means no retry"
        );
    }

    #[test]
    fn backoff_is_shift_capped() {
        // A pathological attempt count must not overflow the shift.
        let c = cfg(true, 100);
        if let RetryDecision::Retry { backoff_ms } = decide("web_search", 50, &failed(), &c) {
            assert_eq!(backoff_ms, 500 * (1u64 << MAX_BACKOFF_SHIFT));
        } else {
            panic!("expected a retry");
        }
    }
}
