use anyhow::Result;
use chrono::Utc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;

use super::db::CronDB;
use super::delivery::{deliver_results, DeliveryOutcome};
use super::types::*;

/// Public wrapper for execute_job, callable from Tauri commands.
pub async fn execute_job_public(
    cron_db: &Arc<CronDB>,
    session_db: &Arc<crate::session::SessionDB>,
    job: &CronJob,
) {
    match cron_db.claim_immediate_job_for_execution(job) {
        Ok(Some(claimed)) => execute_claimed_job(cron_db, session_db, claimed).await,
        Ok(None) => {
            app_warn!(
                "cron",
                "executor",
                "Job '{}' ({}) is already running, skipping",
                job.name,
                job.id
            );
        }
        Err(e) => {
            app_error!(
                "cron",
                "executor",
                "Failed to claim job '{}': {}",
                job.name,
                e
            );
        }
    }
}

/// Panic-safe backstop that releases a cron job's concurrency slot if the run
/// unwinds before reaching one of its normal terminal paths. Without this, a
/// panic anywhere inside `run_chat_engine` would leave `running_at` set until the
/// next process restart — and since §4 counts every `running_at` marker against
/// the global concurrency cap, a handful of leaked markers would permanently
/// starve the cap and stall the whole scheduler. The clear is **owner-checked**
/// (only fires when `running_at` still equals this run's claim timestamp), so on
/// the normal path (already cleared) and after a later re-claim (new timestamp)
/// it harmlessly no-ops.
struct RunningMarkerGuard {
    cron_db: Arc<CronDB>,
    job_id: String,
    claimed_at: String,
    /// §9 (D2): id of the in-progress run log, set once it's inserted (0 until
    /// then). On an abnormal unwind the Drop finalizes it to `error` so a
    /// same-process panic doesn't leave a perpetual `running` row; the
    /// cross-restart backstop is `recover_orphaned_runs`.
    run_log_id: AtomicI64,
}

impl Drop for RunningMarkerGuard {
    fn drop(&mut self) {
        match self
            .cron_db
            .clear_running_if_owner(&self.job_id, &self.claimed_at)
        {
            Ok(true) => {
                app_warn!(
                    "cron",
                    "executor",
                    "Released leaked running marker for job {} (run did not reach a normal terminal path — likely panicked)",
                    self.job_id
                );
                // The run never reached a terminal path, so its in-progress run
                // log is still open — close it out as error.
                let run_log_id = self.run_log_id.load(Ordering::SeqCst);
                if run_log_id > 0 {
                    let _ = self.cron_db.finalize_run_log(
                        run_log_id,
                        "error",
                        &Utc::now().to_rfc3339(),
                        None,
                        None,
                        Some("Interrupted (run did not reach a terminal path)"),
                        None,
                    );
                }
            }
            Ok(false) => {} // normal path already cleared, or re-claimed since
            Err(e) => app_error!(
                "cron",
                "executor",
                "Failed to release running marker for job {}: {}",
                self.job_id,
                e
            ),
        }
    }
}

/// §9 (C7): RAII cleanup of a run's cancel registration. Held for the whole run
/// so every exit path (including the early no-session return and panics) clears
/// the live flag + any unconsumed pending placeholder.
struct CancelRegistrationGuard {
    job_id: String,
}

impl Drop for CancelRegistrationGuard {
    fn drop(&mut self) {
        super::cancel::remove(&self.job_id);
    }
}

/// Execute a job whose running marker was already claimed by the DB.
pub(crate) async fn execute_claimed_job(
    cron_db: &Arc<CronDB>,
    session_db: &Arc<crate::session::SessionDB>,
    claimed: ClaimedCronJob,
) {
    let start_time = std::time::Instant::now();
    let started_at = claimed.claimed_at.clone();
    let job = claimed.job;

    // Panic-safe slot release: held for the whole run, fires only if an abnormal
    // unwind skips the explicit `clear_running` on the terminal paths below.
    let running_guard = RunningMarkerGuard {
        cron_db: cron_db.clone(),
        job_id: job.id.clone(),
        claimed_at: started_at.clone(),
        run_log_id: AtomicI64::new(0),
    };

    // §9 (C7): register the cancel flag immediately after claim — before any
    // session creation / await — so a cancel arriving in the claim→register
    // window isn't silently dropped. Keyed by `started_at` (this run's
    // claimed_at) so `register` only honors a placeholder targeting THIS run;
    // the guard clears it on every exit path.
    let cancel_flag = super::cancel::register(&job.id, &started_at);
    let _cancel_guard = CancelRegistrationGuard {
        job_id: job.id.clone(),
    };

    app_info!(
        "cron",
        "executor",
        "Executing job '{}' ({})",
        job.name,
        job.id
    );

    // Extract prompt and resolve the execution context. Cron sessions are
    // isolated, but can still inherit Project defaults just like a new Project
    // chat when the job is bound to a Project.
    let (prompt, explicit_agent_id) = match &job.payload {
        CronPayload::AgentTurn { prompt, agent_id } => (prompt.clone(), agent_id.as_deref()),
    };
    let context = resolve_execution_context(&job, explicit_agent_id, cron_db);
    let agent_id = context.agent_id;
    let project_id = context.project_id;

    if context.cleared_missing_project {
        app_warn!(
            "cron",
            "executor",
            "Project for job '{}' ({}) no longer exists; cleared project association and running without Project context",
            job.name,
            job.id
        );
    }

    if let Some(pid) = project_id.as_deref() {
        app_info!(
            "cron",
            "executor",
            "Job '{}' ({}) running in project {} with agent {}",
            job.name,
            job.id,
            pid,
            agent_id
        );
    };

    // Create an isolated session for this cron run
    let session_id =
        match session_db.create_session_with_project(&agent_id, project_id.as_deref(), None) {
            Ok(meta) => {
                let _ = session_db.update_session_title(&meta.id, &job.name);
                let _ = session_db.mark_session_cron(&meta.id);
                meta.id
            }
            Err(e) => {
                app_error!(
                    "cron",
                    "executor",
                    "Failed to create session for job '{}': {}",
                    job.name,
                    e
                );
                record_failure(
                    cron_db,
                    &job,
                    &started_at,
                    start_time,
                    "no_session",
                    &e.to_string(),
                    "",
                    None,
                    None,
                );
                return;
            }
        };

    // §9 (D2): open an in-progress run log now that the session exists. A crash
    // mid-run leaves this row open → recover_orphaned_runs closes it as error on
    // the next startup; the running guard finalizes it on a same-process panic;
    // the terminal paths below finalize it to success/error/cancelled.
    let run_log_id = cron_db
        .add_running_run_log(&job.id, &session_id, &started_at)
        .unwrap_or(0);
    running_guard.run_log_id.store(run_log_id, Ordering::SeqCst);

    // Persist the cron prompt before execution so `run_chat_engine` can reuse
    // the same DB contract as interactive chat without duplicating user rows.
    let mut user_msg =
        crate::session::NewMessage::user(&prompt).with_source(crate::chat_engine::ChatSource::Cron);
    user_msg.attachments_meta = Some(
        serde_json::json!({
            "cron_trigger": {
                "job_id": &job.id,
                "job_name": &job.name,
            }
        })
        .to_string(),
    );
    let _ = session_db.append_message(&session_id, &user_msg);

    // Per-run timeout (configurable, clamped to [30, 7200]s) to keep a wedged run
    // from holding its concurrency slot indefinitely (§5).
    let timeout_secs = crate::config::cached_config()
        .cron
        .effective_job_timeout_secs();
    let result = match tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        build_and_run_agent_with_cancel(
            &agent_id,
            &prompt,
            &session_id,
            session_db,
            cancel_flag.clone(),
        ),
    )
    .await
    {
        Ok(r) => r,
        Err(_) => {
            app_error!(
                "cron",
                "executor",
                "Job '{}' timed out after {}s",
                job.name,
                timeout_secs
            );
            Err(anyhow::anyhow!(
                "Cron job timed out after {}s",
                timeout_secs
            ))
        }
    };

    let duration_ms = start_time.elapsed().as_millis() as u64;
    let finished_at = Utc::now().to_rfc3339();
    let was_cancelled = cancel_flag.load(Ordering::SeqCst);

    // §9 (C4): classify the terminal outcome (pure, unit-tested — see
    // `classify_cron_terminal`). The subtlety: cron runs with
    // `abort_on_cancel = false`, so an interrupting cancel does NOT surface as
    // `Err` — the engine swallows it and returns `Ok("")`. So an empty `Ok` with
    // the cancel flag set is a cancellation, while a non-empty `Ok` (including a
    // cancel that landed only after real output) is a genuine success.
    match classify_cron_terminal(&result, was_cancelled) {
        CronTerminal::Cancelled => {
            app_warn!(
                "cron",
                "executor",
                "Job '{}' ({}) cancelled after {}ms",
                job.name,
                job.id,
                duration_ms
            );
            record_cancelled(cron_db, &job, &finished_at, duration_ms, run_log_id);
        }
        CronTerminal::Success => {
            // Classifier returns Success only for `Ok`.
            let response = result.unwrap_or_default();
            app_info!(
                "cron",
                "executor",
                "Job '{}' completed successfully ({}ms)",
                job.name,
                duration_ms
            );

            let preview = if response.len() > 500 {
                Some(crate::truncate_utf8(&response, 500).to_string())
            } else {
                Some(response.clone())
            };
            let _ = cron_db.update_after_run(&job.id, true, &job.schedule);

            // Deliver first so the run log records the delivery outcome (§8) in
            // the same terminal finalize (§9 D2 — the row was opened at start).
            let report = deliver_results(&job, DeliveryOutcome::Success { text: &response }).await;
            let _ = cron_db.finalize_run_log(
                run_log_id,
                "success",
                &finished_at,
                Some(duration_ms),
                preview.as_deref(),
                None,
                report.run_log_status(),
            );

            let _ = cron_db.clear_running(&job.id);

            // Emit Tauri event
            emit_cron_event(&job.id, &job.name, "success", job.notify_on_complete);
        }
        CronTerminal::Failure => {
            // Classifier returns Failure only for `Err`.
            let err_text = result
                .err()
                .map(|e| e.to_string())
                .unwrap_or_else(|| "unknown error".to_string());
            let class = super::failure::CronFailureClass::classify(&err_text);
            app_error!(
                "cron",
                "executor",
                "Job '{}' failed ({}): {}",
                job.name,
                class.key(),
                err_text
            );
            persist_failure_message_if_missing(session_db, &session_id, &err_text);

            // Notify IM channel targets of the failure before bookkeeping.
            let report = deliver_results(&job, DeliveryOutcome::Failure { error: &err_text }).await;

            record_failure(
                cron_db,
                &job,
                &started_at,
                start_time,
                class.run_log_status(),
                &err_text,
                &session_id,
                report.run_log_status(),
                Some(run_log_id),
            );
        }
    }
}

/// §9 (C4): the terminal disposition of a cron run.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CronTerminal {
    Success,
    Cancelled,
    Failure,
}

/// Classify a cron run's `(result, was_cancelled)` into its terminal action.
/// Pure so the decision table — including the `abort_on_cancel = false` quirk
/// where an interrupting cancel returns `Ok("")` rather than `Err` — is
/// unit-testable without standing up the engine.
pub(crate) fn classify_cron_terminal(result: &Result<String>, was_cancelled: bool) -> CronTerminal {
    match result {
        // Interrupted run: the engine swallowed the cancel (abort_on_cancel=false)
        // and returned an empty Ok. Not a success — don't deliver a blank or
        // advance the schedule.
        Ok(r) if was_cancelled && r.trim().is_empty() => CronTerminal::Cancelled,
        // Genuine output (incl. a cancel that arrived only after real output).
        Ok(_) => CronTerminal::Success,
        // Defensive: only reached if a caller flips abort_on_cancel=true so a
        // cancel surfaces as Err.
        Err(_) if was_cancelled => CronTerminal::Cancelled,
        Err(_) => CronTerminal::Failure,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CronExecutionContext {
    pub agent_id: String,
    pub project_id: Option<String>,
    pub cleared_missing_project: bool,
}

pub(crate) fn resolve_execution_context(
    job: &CronJob,
    explicit_agent_id: Option<&str>,
    cron_db: &Arc<CronDB>,
) -> CronExecutionContext {
    let trimmed_explicit = explicit_agent_id.and_then(|id| {
        let trimmed = id.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    });

    let mut cleared_missing_project = false;
    let project = job
        .project_id
        .as_deref()
        .and_then(|pid| match crate::get_project_db() {
            Some(db) => match db.get(pid) {
                Ok(Some(project)) => Some(project),
                Ok(None) => {
                    match cron_db.clear_job_project(&job.id) {
                        Ok(()) => cleared_missing_project = true,
                        Err(e) => app_warn!(
                            "cron",
                            "executor",
                            "Failed to clear missing project {} from job {}: {}",
                            pid,
                            job.id,
                            e
                        ),
                    }
                    None
                }
                Err(e) => {
                    app_warn!(
                        "cron",
                        "executor",
                        "Failed to load project {} for job {}: {}",
                        pid,
                        job.id,
                        e
                    );
                    None
                }
            },
            None => {
                app_warn!(
                    "cron",
                    "executor",
                    "Project DB not initialized while resolving project {} for job {}",
                    pid,
                    job.id
                );
                None
            }
        });

    let agent_id = resolve_agent_id_for_execution(trimmed_explicit, project.as_ref());

    CronExecutionContext {
        agent_id,
        project_id: project.map(|p| p.id),
        cleared_missing_project,
    }
}

pub(crate) fn resolve_agent_id_for_execution(
    explicit_agent_id: Option<&str>,
    project: Option<&crate::project::Project>,
) -> String {
    crate::agent::resolver::resolve_default_agent_id_full(
        explicit_agent_id,
        project,
        None,
        None,
        None,
        None,
    )
    .0
}

/// Build an AssistantAgent and run a chat message with full failover logic.
///
/// Cron now delegates to the shared chat engine so provider auth, Codex OAuth,
/// failover, compaction, and persistence stay aligned with interactive chat.
pub async fn build_and_run_agent_with_cancel(
    agent_id: &str,
    message: &str,
    session_id: &str,
    session_db: &Arc<crate::session::SessionDB>,
    cancel: Arc<AtomicBool>,
) -> Result<String> {
    build_and_run_agent_with_context(
        agent_id,
        message,
        session_id,
        session_db,
        None,
        Some(cancel),
    )
    .await
}

/// Build an AssistantAgent and run a chat message via the shared chat engine
/// with optional extra system context.
pub async fn build_and_run_agent_with_context(
    agent_id: &str,
    message: &str,
    session_id: &str,
    session_db: &Arc<crate::session::SessionDB>,
    extra_system_context: Option<&str>,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<String> {
    use crate::provider;

    // Load app config from disk
    let store = crate::config::cached_config();

    // Load agent config for model resolution
    let agent_model_config = crate::agent_loader::load_agent(agent_id)
        .map(|def| def.config.model)
        .unwrap_or_default();

    let (primary, fallbacks) = provider::resolve_model_chain(&agent_model_config, &store);

    // Build model chain
    let mut model_chain = Vec::new();
    if let Some(p) = primary {
        model_chain.push(p);
    }
    for fb in fallbacks {
        if !model_chain
            .iter()
            .any(|m| m.provider_id == fb.provider_id && m.model_id == fb.model_id)
        {
            model_chain.push(fb);
        }
    }

    if model_chain.is_empty() {
        return Err(anyhow::anyhow!(
            "No model configured for cron job execution"
        ));
    }

    let agent_def = crate::agent_loader::load_agent(agent_id).ok();
    let engine_params = crate::chat_engine::ChatEngineParams {
        session_id: session_id.to_string(),
        agent_id: agent_id.to_string(),
        turn_id: None,
        message: message.to_string(),
        display_text: None,
        attachments: Vec::new(),
        session_db: session_db.clone(),
        model_chain,
        providers: store.providers.clone(),
        codex_token: None,
        resolved_temperature: agent_def
            .as_ref()
            .and_then(|def| def.config.model.temperature)
            .or(store.temperature),
        compact_config: store.compact.clone(),
        extra_system_context: Some(
            extra_system_context
                .unwrap_or(
                    "## Execution Context\n\
                 You are running as a **scheduled task** (cron job), not an interactive chat.\n\
                 - No user is actively waiting — execute the prompt directly and concisely.\n\
                 - This is an isolated session with no prior conversation history.\n\
                 - Focus on completing the task described in the user message.",
                )
                .to_string(),
        ),
        reasoning_effort: agent_def
            .as_ref()
            .and_then(|def| def.config.model.reasoning_effort.clone())
            .or(crate::agent::live_reasoning_effort(None).await),
        cancel: cancel.unwrap_or_else(|| Arc::new(AtomicBool::new(false))),
        plan_context_override: None,
        skill_allowed_tools: Vec::new(),
        denied_tools: Vec::new(),
        tool_scope: None,
        subagent_depth: 0,
        steer_run_id: None,
        auto_approve_tools: false,
        follow_global_reasoning_effort: false,
        post_turn_effects: true,
        abort_on_cancel: false,
        persist_final_error_event: true,
        // Cron is a background/non-interactive runner, but owner-internal: it
        // holds the foreground idle guard and gets owner-plane KB access (maps to
        // `KbAccessSource::Cron`, NOT the IM cap). `origin_source: None` lets the
        // engine derive the origin from `source`, so a subagent spawned by this
        // cron run inherits the non-IM `Cron` origin and isn't WS8-denied.
        source: crate::chat_engine::stream_seq::ChatSource::Cron,
        origin_source: None,
        channel_kb_context: None,
        event_sink: Arc::new(crate::chat_engine::NoopEventSink),
    };

    match crate::chat_engine::run_chat_engine(engine_params).await {
        Ok(result) => Ok(result.response),
        Err(e) => Err(anyhow::anyhow!("{}", e)),
    }
}

pub fn cancel_running_job(job_id: &str) -> Result<Option<bool>> {
    let Some(cron_db) = crate::get_cron_db() else {
        return Ok(None);
    };
    let Some(job) = cron_db.get_job(job_id)? else {
        return Ok(None);
    };
    let Some(running_at) = job.running_at.as_deref() else {
        return Ok(Some(false));
    };
    // §9 (C7): key the cancel to this run's claim timestamp so a placeholder
    // left in the claim→register window can't leak onto a later run (see
    // `cancel.rs`). `running_at` IS the in-flight run's `claimed_at`.
    Ok(Some(super::cancel::cancel(job_id, running_at)))
}

fn persist_failure_message_if_missing(
    session_db: &Arc<crate::session::SessionDB>,
    session_id: &str,
    err_text: &str,
) {
    let already_persisted = session_db
        .load_session_messages_latest(session_id, 1)
        .ok()
        .and_then(|(msgs, _, _)| msgs.last().cloned())
        .map(|msg| msg.content == err_text)
        .unwrap_or(false);

    if already_persisted {
        return;
    }

    let mut err_msg = crate::session::NewMessage::assistant(err_text)
        .with_source(crate::chat_engine::ChatSource::Cron);
    err_msg.is_error = Some(true);
    let _ = session_db.append_message(session_id, &err_msg);
}

/// Record a failure run log and update job state. §9 (D2): when `run_log_id` is
/// `Some` (the normal path, where an in-progress row was opened at run start),
/// finalize that row; when `None` (the no-session early failure, which never
/// opened one) insert a complete row.
#[allow(clippy::too_many_arguments)]
pub(crate) fn record_failure(
    cron_db: &Arc<CronDB>,
    job: &CronJob,
    started_at: &str,
    start_time: std::time::Instant,
    status: &str,
    error: &str,
    session_id: &str,
    delivery_status: Option<&str>,
    run_log_id: Option<i64>,
) {
    let duration_ms = start_time.elapsed().as_millis() as u64;
    let finished_at = Utc::now().to_rfc3339();

    match run_log_id {
        Some(id) => {
            let _ = cron_db.finalize_run_log(
                id,
                status,
                &finished_at,
                Some(duration_ms),
                None,
                Some(error),
                delivery_status,
            );
        }
        None => {
            let run_log = CronRunLog {
                id: 0,
                job_id: job.id.clone(),
                session_id: session_id.to_string(),
                status: status.to_string(),
                started_at: started_at.to_string(),
                finished_at: Some(finished_at),
                duration_ms: Some(duration_ms),
                result_preview: None,
                error: Some(error.to_string()),
                delivery_status: delivery_status.map(|s| s.to_string()),
            };
            let _ = cron_db.add_run_log(&run_log);
        }
    }
    let auto_disabled = cron_db
        .update_after_run(&job.id, false, &job.schedule)
        .unwrap_or(false);
    let _ = cron_db.clear_running(&job.id);

    if auto_disabled {
        // The job just crossed its max_failures threshold and was disabled.
        // Always notify (overriding notify_on_complete) — a silently dead
        // scheduled task is exactly the failure mode this surfaces (§5).
        let consecutive = job.consecutive_failures.saturating_add(1);
        let reason = super::failure::CronFailureClass::classify(error).key();
        app_warn!(
            "cron",
            "executor",
            "Job '{}' ({}) auto-disabled after {} consecutive failures (last: {})",
            job.name,
            job.id,
            consecutive,
            reason
        );
        emit_cron_disabled_event(&job.id, &job.name, consecutive, reason);
    } else {
        emit_cron_event(&job.id, &job.name, "error", job.notify_on_complete);
    }
}

/// §9 (D2): finalize the in-progress run log as cancelled. Always has a
/// `run_log_id` — cancellation only reaches here after the run started (the row
/// was opened right after session creation).
fn record_cancelled(
    cron_db: &Arc<CronDB>,
    job: &CronJob,
    finished_at: &str,
    duration_ms: u64,
    run_log_id: i64,
) {
    let _ = cron_db.finalize_run_log(
        run_log_id,
        "cancelled",
        finished_at,
        Some(duration_ms),
        None,
        Some("Cancelled by user"),
        None,
    );
    let _ = cron_db.clear_running(&job.id);
    emit_cron_event(&job.id, &job.name, "cancelled", job.notify_on_complete);
}

/// Emit an event to notify the frontend of a cron run result.
pub(crate) fn emit_cron_event(job_id: &str, job_name: &str, status: &str, notify: bool) {
    if let Some(bus) = crate::get_event_bus() {
        let payload = serde_json::json!({
            "job_id": job_id,
            "job_name": job_name,
            "status": status,
            "notify": notify,
        });
        bus.emit("cron:run_completed", payload);
    }
}

/// Emit the one-shot "job auto-disabled" signal (§5). Rides the same
/// `cron:run_completed` channel the frontend already listens on, but forces
/// `notify=true` and carries `auto_disabled` + the consecutive-failure count +
/// the failure-reason key so the GUI shows a distinct, always-on notification
/// regardless of the job's `notify_on_complete` preference.
pub(crate) fn emit_cron_disabled_event(
    job_id: &str,
    job_name: &str,
    consecutive_failures: u32,
    reason_key: &str,
) {
    if let Some(bus) = crate::get_event_bus() {
        let payload = serde_json::json!({
            "job_id": job_id,
            "job_name": job_name,
            "status": "error",
            "notify": true,
            "auto_disabled": true,
            "consecutive_failures": consecutive_failures,
            "failure_reason": reason_key,
        });
        bus.emit("cron:run_completed", payload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cron::{CronPayload, CronSchedule, NewCronJob};
    use crate::project::Project;
    use rusqlite::params;
    use std::path::{Path, PathBuf};
    use uuid::Uuid;

    #[test]
    fn classify_cron_terminal_decision_table() {
        // Genuine success.
        assert_eq!(
            classify_cron_terminal(&Ok("hi".into()), false),
            CronTerminal::Success
        );
        // Empty Ok without a cancel = still success (the §10 empty-output case,
        // intentionally unchanged here).
        assert_eq!(
            classify_cron_terminal(&Ok(String::new()), false),
            CronTerminal::Success
        );
        // §9 (C4) core: cron's engine runs abort_on_cancel=false, so an
        // interrupting cancel returns Ok("") — must classify as Cancelled, not a
        // blank "success".
        assert_eq!(
            classify_cron_terminal(&Ok(String::new()), true),
            CronTerminal::Cancelled
        );
        assert_eq!(
            classify_cron_terminal(&Ok("   \n".into()), true),
            CronTerminal::Cancelled
        );
        // A cancel that landed only AFTER real output → honor the completed work.
        assert_eq!(
            classify_cron_terminal(&Ok("done".into()), true),
            CronTerminal::Success
        );
        // Genuine failure vs. a cancel surfacing as Err (defensive path).
        assert_eq!(
            classify_cron_terminal(&Err(anyhow::anyhow!("boom")), false),
            CronTerminal::Failure
        );
        assert_eq!(
            classify_cron_terminal(&Err(anyhow::anyhow!("interrupted")), true),
            CronTerminal::Cancelled
        );
    }

    fn temp_db_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "hope-agent-cron-executor-{label}-{}.db",
            Uuid::new_v4()
        ))
    }

    fn cleanup_db_files(path: &Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    fn project_with_default_agent(agent_id: Option<&str>) -> Project {
        Project {
            id: "project-1".into(),
            name: "Project One".into(),
            description: None,
            instructions: None,
            emoji: None,
            logo: None,
            color: None,
            default_agent_id: agent_id.map(str::to_string),
            default_model_id: None,
            working_dir: None,
            created_at: 0,
            updated_at: 0,
            archived: false,
        }
    }

    #[test]
    fn resolve_agent_id_for_execution_prefers_explicit_agent() {
        let project = project_with_default_agent(Some("project-agent"));
        let resolved = resolve_agent_id_for_execution(Some("explicit-agent"), Some(&project));
        assert_eq!(resolved, "explicit-agent");
    }

    #[test]
    fn resolve_agent_id_for_execution_uses_project_default_agent() {
        let project = project_with_default_agent(Some("project-agent"));
        let resolved = resolve_agent_id_for_execution(None, Some(&project));
        assert_eq!(resolved, "project-agent");
    }

    #[test]
    fn resolve_agent_id_for_execution_falls_back_without_project_default() {
        let project = project_with_default_agent(None);
        let resolved = resolve_agent_id_for_execution(None, Some(&project));
        assert!(!resolved.trim().is_empty());
    }

    #[test]
    fn record_cancelled_writes_log_clears_running_and_preserves_failures() {
        let path = temp_db_path("cancelled-log");
        let db = Arc::new(CronDB::open(&path).expect("open db"));
        let job = db
            .add_job(&NewCronJob {
                name: "Hydrate".into(),
                description: None,
                project_id: None,
                schedule: CronSchedule::Every {
                    interval_ms: 300_000,
                    start_at: None,
                },
                payload: CronPayload::AgentTurn {
                    prompt: "drink water".into(),
                    agent_id: None,
                },
                max_failures: Some(5),
                notify_on_complete: Some(false),
                delivery_targets: None,
                prefix_delivery_with_name: None,
            })
            .expect("add job");
        {
            let conn = db.conn.lock().expect("lock");
            conn.execute(
                "UPDATE cron_jobs SET consecutive_failures=2 WHERE id=?1",
                params![job.id],
            )
            .expect("seed failures");
        }
        let claimed = db
            .claim_immediate_job_for_execution(&job)
            .expect("claim")
            .expect("claimed job");

        // §9 (D2): cancellation finalizes an already-open in-progress run log.
        let run_log_id = db
            .add_running_run_log(&job.id, "session-cancel", &claimed.claimed_at)
            .expect("open in-progress run log");
        record_cancelled(&db, &claimed.job, "2026-01-01T00:00:42Z", 42, run_log_id);

        let stored = db.get_job(&job.id).expect("load").expect("job exists");
        assert!(stored.running_at.is_none());
        assert_eq!(stored.consecutive_failures, 2);
        let logs = db.get_run_logs(&job.id, 10).expect("logs");
        assert_eq!(
            logs.len(),
            1,
            "in-progress row finalized in place, no duplicate"
        );
        assert_eq!(logs[0].status, "cancelled");
        assert_eq!(logs[0].session_id, "session-cancel");
        assert_eq!(logs[0].duration_ms, Some(42));
        assert_eq!(logs[0].error.as_deref(), Some("Cancelled by user"));

        cleanup_db_files(&path);
    }
}
