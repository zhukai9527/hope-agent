use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

/// Live cancel flags for jobs that have already registered (i.e. their run has
/// reached `execute_claimed_job` and called [`register`]).
static CANCELS: LazyLock<Mutex<HashMap<String, Arc<AtomicBool>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// §9 (C7): pending cancels for jobs claimed (running_at set) but whose run
/// hasn't reached [`register`] yet. **Keyed by the run's `claimed_at`**, not a
/// bare job id: `cancel_running_job` reads `running_at` and then (after a TOCTOU
/// gap) calls [`cancel`], so the in-flight run could finish in between and a
/// later run of a *recurring* job could otherwise inherit the stale placeholder.
/// Recording the claim timestamp means [`register`] only honors a placeholder
/// that targets *this* run (`pending_claimed_at == claimed_at`); a placeholder
/// left by a since-finished run is drained but ignored, so it can never cancel a
/// different run. [`remove`] clears the entry at run end.
static PENDING_CANCELS: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Register a cancel flag for a starting run, identified by its `claimed_at`. If
/// a cancel for *this* run arrived during the claim→register window (a pending
/// placeholder keyed to the same `claimed_at`), the flag starts already set so
/// the run is cancelled at its first checkpoint. A placeholder for a different
/// (earlier, since-finished) run is drained but does not set the flag.
pub(crate) fn register(job_id: &str, claimed_at: &str) -> Arc<AtomicBool> {
    let targets_this_run = {
        let mut pending = PENDING_CANCELS.lock().unwrap_or_else(|p| p.into_inner());
        // Always drain (clears stale placeholders); honor only on an exact match.
        match pending.remove(job_id) {
            Some(pending_claimed_at) => pending_claimed_at == claimed_at,
            None => false,
        }
    };
    let flag = Arc::new(AtomicBool::new(targets_this_run));
    {
        let mut map = CANCELS.lock().unwrap_or_else(|p| p.into_inner());
        map.insert(job_id.to_string(), flag.clone());
    }
    flag
}

/// Request cancellation of the run identified by `claimed_at`. Returns `true` if
/// the request was recorded — either by flipping a live flag, or (during the
/// claim→register window) by leaving a run-keyed pending placeholder that
/// [`register`] will pick up only for the matching run.
pub(crate) fn cancel(job_id: &str, claimed_at: &str) -> bool {
    {
        let map = CANCELS.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(flag) = map.get(job_id) {
            flag.store(true, Ordering::SeqCst);
            return true;
        }
    }
    // No live flag yet: the run is claimed but hasn't registered. Record a
    // run-keyed pending cancel for `register` to drain.
    let mut pending = PENDING_CANCELS.lock().unwrap_or_else(|p| p.into_inner());
    pending.insert(job_id.to_string(), claimed_at.to_string());
    true
}

/// Clear a run's cancel state at terminal. Removes both the live flag and any
/// pending placeholder for the job so nothing leaks into a later run.
pub(crate) fn remove(job_id: &str) {
    {
        let mut map = CANCELS.lock().unwrap_or_else(|p| p.into_inner());
        map.remove(job_id);
    }
    let mut pending = PENDING_CANCELS.lock().unwrap_or_else(|p| p.into_inner());
    pending.remove(job_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_cancel_before_register_starts_run_cancelled() {
        let job = "job-pending";
        // Cancel arrives in the claim→register window for run "ts-1".
        assert!(cancel(job, "ts-1"), "cancel records a pending placeholder");
        // The same run then registers and must observe the cancel immediately.
        let flag = register(job, "ts-1");
        assert!(
            flag.load(Ordering::SeqCst),
            "register drained pending cancel"
        );
        remove(job);
    }

    #[test]
    fn live_cancel_flips_registered_flag() {
        let job = "job-live";
        let flag = register(job, "ts-1");
        assert!(!flag.load(Ordering::SeqCst));
        assert!(cancel(job, "ts-1"));
        assert!(flag.load(Ordering::SeqCst));
        remove(job);
    }

    #[test]
    fn stale_pending_for_a_finished_run_does_not_cancel_a_later_run() {
        // Regression guard for the §9 review finding: a cancel that lands after
        // run "ts-1" already finished (its remove() ran) leaves a placeholder
        // keyed to "ts-1"; the NEXT run "ts-2" of a recurring job must NOT be
        // cancelled by it.
        let job = "job-recurring";
        remove(job); // run ts-1 finished, cleared its state
        assert!(
            cancel(job, "ts-1"),
            "delayed cancel records ts-1 placeholder"
        );
        let flag = register(job, "ts-2"); // a different, later run starts
        assert!(
            !flag.load(Ordering::SeqCst),
            "stale ts-1 placeholder must not cancel run ts-2"
        );
        remove(job);
    }

    #[test]
    fn remove_clears_unconsumed_pending() {
        let job = "job-leak";
        assert!(cancel(job, "ts-1"));
        remove(job);
        let flag = register(job, "ts-1");
        assert!(
            !flag.load(Ordering::SeqCst),
            "remove cleared the placeholder before register could drain it"
        );
        remove(job);
    }
}
