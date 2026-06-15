//! Concurrency scheduler for backgrounded tool jobs (MISC-5 + R7.1 queue).
//!
//! When the configured cap (`async_tools.max_concurrent_jobs`) is full, a new
//! background job no longer hard-rejects — it QUEUES (status `Queued`) and a
//! **Primary-only** scheduler task promotes queued jobs per-session
//! round-robin as slots free. The queue holds each job's live
//! [`ToolExecContext`] in memory (it cannot be persisted), so queued jobs do
//! NOT survive a restart — they are recovered as `Interrupted` like running
//! jobs (see `replay_pending_jobs`).
//!
//! Accounting model: every *running* job holds exactly one [`SlotReservation`],
//! granted by [`try_reserve`] (immediate path) or [`try_take_next`] (promote
//! path). Its `Drop` decrements the counts and wakes the scheduler, so a freed
//! slot immediately pulls the next queued job. `max_concurrent_jobs == 0` means
//! unlimited (no cap, no queueing).
//!
//! Fairness: [`pick_fair_index`] picks the queued job whose session currently
//! has the FEWEST running jobs (ties → oldest), so one session (or IM chat)
//! cannot monopolize the pool while others wait.

use std::collections::{HashMap, VecDeque};
use std::sync::{LazyLock, Mutex};

use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::tools::ToolExecContext;

/// Hard ceiling on the in-memory wait queue. Each queued job pins its
/// `ToolExecContext` in RAM, so the queue must be bounded; past this a new
/// background request hard-rejects (the model waits / runs synchronously).
/// Internal guardrail (not a user knob) — high enough to never bite normal use,
/// low enough to bound memory under a runaway model.
const MAX_QUEUED_JOBS: usize = 256;

/// Everything needed to start a backgrounded job later, parked in the queue
/// while it waits for a slot. Holds the live ctx (not persistable), so it lives
/// only in the Primary process's memory.
pub struct PreparedJob {
    pub job_id: String,
    pub tool_name: String,
    pub args: Value,
    pub ctx: ToolExecContext,
    pub max_secs: u64,
    pub preview_bytes: usize,
    pub cancel_token: CancellationToken,
}

impl PreparedJob {
    fn session_key(&self) -> String {
        self.ctx.session_id.clone().unwrap_or_default()
    }
}

/// Per-session round-robin: index of the queued job whose session currently has
/// the fewest running jobs; ties broken by queue position (oldest first).
/// Pure (no globals) so the fairness rule is unit-testable. `None` ⇒ empty.
fn pick_fair_index<'a>(
    queue_sessions: impl Iterator<Item = &'a str>,
    per_session: &HashMap<String, usize>,
) -> Option<usize> {
    let mut best: Option<(usize, usize)> = None; // (index, running_count)
    for (i, sess) in queue_sessions.enumerate() {
        let running = *per_session.get(sess).unwrap_or(&0);
        match best {
            Some((_, best_running)) if running >= best_running => {}
            _ => best = Some((i, running)),
        }
    }
    best.map(|(i, _)| i)
}

struct SlotManager {
    total: usize,
    per_session: HashMap<String, usize>,
    queue: VecDeque<PreparedJob>,
}

impl SlotManager {
    fn new() -> Self {
        Self {
            total: 0,
            per_session: HashMap::new(),
            queue: VecDeque::new(),
        }
    }

    /// Reserve a slot if there is room (`cap == 0` ⇒ unlimited). Increments the
    /// counts and returns true; false ⇒ caller should enqueue.
    fn reserve_inner(&mut self, cap: usize, session_key: &str) -> bool {
        if cap != 0 && self.total >= cap {
            return false;
        }
        self.total += 1;
        *self.per_session.entry(session_key.to_string()).or_insert(0) += 1;
        true
    }

    /// Pick + remove the next queued job if a slot is free, incrementing counts.
    fn take_next_inner(&mut self, cap: usize) -> Option<PreparedJob> {
        if cap != 0 && self.total >= cap {
            return None;
        }
        let idx = pick_fair_index(
            self.queue.iter().map(|j| j.session_key_ref()),
            &self.per_session,
        )?;
        let job = self.queue.remove(idx)?;
        self.total += 1;
        *self.per_session.entry(job.session_key()).or_insert(0) += 1;
        Some(job)
    }

    /// Release a running slot.
    fn release(&mut self, session_key: &str) {
        self.total = self.total.saturating_sub(1);
        if let Some(c) = self.per_session.get_mut(session_key) {
            *c = c.saturating_sub(1);
            if *c == 0 {
                self.per_session.remove(session_key);
            }
        }
    }
}

impl PreparedJob {
    /// Borrowed session key for fairness scanning without allocation.
    fn session_key_ref(&self) -> &str {
        self.ctx.session_id.as_deref().unwrap_or("")
    }
}

static MANAGER: LazyLock<Mutex<SlotManager>> = LazyLock::new(|| Mutex::new(SlotManager::new()));
static SCHED_NOTIFY: LazyLock<tokio::sync::Notify> = LazyLock::new(tokio::sync::Notify::new);

fn cap() -> usize {
    crate::config::cached_config()
        .async_tools
        .max_concurrent_jobs
}

fn lock() -> std::sync::MutexGuard<'static, SlotManager> {
    MANAGER.lock().unwrap_or_else(|p| p.into_inner())
}

/// RAII reservation for one running slot. `Drop` decrements the counts and wakes
/// the scheduler, so a freed slot immediately promotes the next queued job. The
/// runner thread holds this for the job's whole lifetime (panic-safe release).
#[must_use = "dropping the reservation releases the slot immediately"]
pub struct SlotReservation {
    session_key: String,
}

impl Drop for SlotReservation {
    fn drop(&mut self) {
        lock().release(&self.session_key);
        SCHED_NOTIFY.notify_one();
    }
}

/// Try to reserve a slot for immediate execution. `None` ⇒ the cap is full and
/// the caller should [`enqueue`] instead. `cap == 0` ⇒ unlimited (always `Some`).
pub fn try_reserve(session_key: &str) -> Option<SlotReservation> {
    let mut m = lock();
    // FIFO fairness: if jobs are already waiting, a fresh spawn must queue behind
    // them rather than jump the line — even when a slot is technically free. The
    // scheduler drains the queue (per-session round-robin) into freed slots.
    if !m.queue.is_empty() {
        return None;
    }
    if m.reserve_inner(cap(), session_key) {
        Some(SlotReservation {
            session_key: session_key.to_string(),
        })
    } else {
        None
    }
}

/// Park a job in the wait queue. `false` ⇒ the queue itself is full (the job is
/// dropped here; the caller hard-rejects and cleans up its row/token by id).
pub fn enqueue(job: PreparedJob) -> bool {
    let mut m = lock();
    if m.queue.len() >= MAX_QUEUED_JOBS {
        return false;
    }
    m.queue.push_back(job);
    true
}

/// Scheduler step: if a slot is free and the queue is non-empty, pick the next
/// job per-session round-robin, reserve its slot, and return it for the
/// scheduler to start. `None` ⇒ nothing to promote right now.
pub fn try_take_next() -> Option<(PreparedJob, SlotReservation)> {
    let mut m = lock();
    let job = m.take_next_inner(cap())?;
    let session_key = job.session_key();
    Some((job, SlotReservation { session_key }))
}

/// Remove a still-queued job by id (used by cancel). `Some` ⇒ it was waiting in
/// the queue and is now removed (no slot was held); `None` ⇒ not queued (already
/// running, already taken by the scheduler, or never queued).
pub fn remove_queued(job_id: &str) -> Option<PreparedJob> {
    let mut m = lock();
    let pos = m.queue.iter().position(|j| j.job_id == job_id)?;
    m.queue.remove(pos)
}

/// Remove ALL still-queued jobs for a session (incognito burn / session purge):
/// drops their in-memory `PreparedJob`s — and the live ctx those pin (which may
/// carry the incognito session's sensitive args) — not just the DB rows. Returns
/// how many were removed. A dropped queued job is never promoted (its row is also
/// being deleted by the caller).
pub fn remove_queued_for_session(session_id: &str) -> usize {
    let mut m = lock();
    let before = m.queue.len();
    m.queue.retain(|j| j.session_key_ref() != session_id);
    before - m.queue.len()
}

/// Wait for the next "slot freed / job enqueued" wakeup (the scheduler task
/// parks here). `Notify` stores one permit, so a wake that arrives between
/// drains is not lost.
pub async fn scheduler_notified() {
    SCHED_NOTIFY.notified().await;
}

/// Wake the scheduler — call after enqueueing so a currently-free slot promotes
/// the new job even though nothing finished.
pub fn wake_scheduler() {
    SCHED_NOTIFY.notify_one();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_fair_index_prefers_session_with_fewest_running() {
        // queue sessions: [A, A, B]; A has 2 running, B has 0 → pick B (idx 2).
        let mut per = HashMap::new();
        per.insert("A".to_string(), 2usize);
        per.insert("B".to_string(), 0usize);
        let q = ["A", "A", "B"];
        assert_eq!(pick_fair_index(q.iter().copied(), &per), Some(2));
    }

    #[test]
    fn pick_fair_index_breaks_ties_by_oldest() {
        // No one running → all tie at 0 → oldest (idx 0) wins.
        let per = HashMap::new();
        let q = ["A", "B", "C"];
        assert_eq!(pick_fair_index(q.iter().copied(), &per), Some(0));
    }

    #[test]
    fn pick_fair_index_empty_is_none() {
        let per = HashMap::new();
        let q: [&str; 0] = [];
        assert_eq!(pick_fair_index(q.iter().copied(), &per), None);
    }

    #[test]
    fn reserve_respects_cap_and_release_frees() {
        let mut m = SlotManager::new();
        assert!(m.reserve_inner(2, "a"));
        assert!(m.reserve_inner(2, "b"));
        assert!(!m.reserve_inner(2, "c"), "cap 2 is full");
        m.release("a");
        assert!(m.reserve_inner(2, "c"), "slot freed → room again");
        assert_eq!(m.total, 2);
    }

    #[test]
    fn reserve_cap_zero_is_unlimited() {
        let mut m = SlotManager::new();
        for i in 0..100 {
            assert!(m.reserve_inner(0, &format!("s{i}")), "cap 0 never blocks");
        }
        assert_eq!(m.total, 100);
    }

    #[test]
    fn release_unknown_session_is_saturating_noop() {
        let mut m = SlotManager::new();
        m.release("never-reserved"); // must not underflow
        assert_eq!(m.total, 0);
    }
}
