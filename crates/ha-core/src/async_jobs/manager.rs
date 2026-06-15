//! `JobManager` — the single entry point for background jobs (R1).
//!
//! Every production code path that spawns / cancels / lists / inspects /
//! recovers a background job goes through this facade rather than the loose
//! module functions. Today only [`JobKind::Tool`] is wired for execution;
//! `Subagent` (R6) and `Group` (R5) will extend `JobManager` with their own
//! spawn entries instead of inventing parallel subsystem APIs — unifying every
//! kind behind one type (one lifecycle, one persistence, one cancel/list/inject
//! surface) is the whole point of the R1 model.
//!
//! The facade is a thin delegator over the proven `async_jobs` internals (the
//! Tool executor): spawn → [`spawn`](super::spawn), cancel/cleanup/replay →
//! [`mod`](super), scheduling → [`spawn::run_scheduler`]. It owns no state; the
//! DB handle stays a process-global [`OnceLock`](super::ASYNC_JOBS_DB) set at
//! init. Reads route through `get` / `list_active_by_session`; the raw DB stays
//! `pub(crate)` for bootstrap (`app_init`) and white-box tests only.

use anyhow::Result;
use serde_json::Value;

use super::types::{BackgroundJob, JobOrigin};
use crate::tools::ToolExecContext;

/// Single entry point for background-job operations (R1). Zero-sized; all
/// methods are associated functions delegating to the shared internals.
pub struct JobManager;

impl JobManager {
    // ── Spawn (Tool executor — the only wired kind in R1) ──────────────────

    /// Spawn an explicit background tool job (`run_in_background: true` or an
    /// agent `always-background` policy). Returns the synthetic started result
    /// JSON the model receives immediately. R6/R5 add `spawn_subagent` /
    /// `spawn_group` here.
    pub fn spawn_tool(
        tool_name: &str,
        args: Value,
        ctx: ToolExecContext,
        origin: JobOrigin,
    ) -> Result<String> {
        super::spawn::spawn_explicit_job(tool_name, args, ctx, origin)
    }

    /// Run a tool synchronously but auto-background it if it exceeds the budget
    /// (`config.async_tools.auto_background_secs`). Returns the inline result if
    /// it finished in time, else the synthetic started result.
    pub async fn dispatch_tool_with_auto_background(
        name: &str,
        args: &Value,
        ctx: &ToolExecContext,
        auto_bg_secs: u64,
    ) -> Result<String> {
        super::spawn::dispatch_with_auto_background(name, args, ctx, auto_bg_secs).await
    }

    // ── Reads (any kind) ───────────────────────────────────────────────────

    /// Load a single job snapshot by id. `Ok(None)` if the job is unknown or the
    /// DB is not initialized.
    pub fn get(job_id: &str) -> Result<Option<BackgroundJob>> {
        match super::get_async_jobs_db() {
            Some(db) => db.load(job_id),
            None => Ok(None),
        }
    }

    /// All active (`queued`/`running`/`cancelling`/`awaiting_approval`) jobs
    /// owned by a session. Empty when the DB is not initialized.
    pub fn list_active_by_session(session_id: &str) -> Result<Vec<BackgroundJob>> {
        match super::get_async_jobs_db() {
            Some(db) => db.list_active_by_session(session_id),
            None => Ok(Vec::new()),
        }
    }

    // ── Cancellation / cleanup ─────────────────────────────────────────────

    /// Best-effort cancel a single job (in-process token + cross-process DB
    /// flag). Returns the updated snapshot, or `None` if the job is unknown.
    pub fn cancel(job_id: &str) -> Result<Option<BackgroundJob>> {
        super::cancel_job(job_id)
    }

    /// Cancel every active job owned by a session (session delete / DELETE-4).
    /// Returns the number cancelled.
    pub fn cancel_for_session(session_id: &str) -> usize {
        super::cancel_jobs_for_session(session_id)
    }

    /// Physically delete all job rows + spool files for a session (incognito
    /// burn-on-close, INCOG-2). Returns the number of rows deleted.
    pub fn purge_for_session(session_id: &str) -> u64 {
        super::purge_jobs_for_session(session_id)
    }

    // ── Recovery / scheduling lifecycle ────────────────────────────────────

    /// Startup recovery (Primary-only): mark interrupted survivors + re-dispatch
    /// terminal-but-uninjected jobs to their parent sessions.
    pub fn replay_pending() {
        super::replay_pending_jobs()
    }

    /// The per-process (tier-agnostic) queue scheduler loop — promotes queued
    /// jobs into freed slots. Idempotent: at most one loop runs per process.
    pub async fn run_scheduler() {
        super::spawn::run_scheduler().await
    }

    /// Spawn the retention sweep loop (Primary-only): purges aged terminal rows
    /// + orphan spool files. No-op ticker if retention is disabled.
    pub fn spawn_retention_loop() {
        super::retention::spawn_background_loop()
    }
}
