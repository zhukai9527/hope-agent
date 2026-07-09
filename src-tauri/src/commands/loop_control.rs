use crate::commands::CmdError;
use ha_core::loop_control::{
    CreateLoopScheduleInput, LoopExecutionStrategy, LoopSchedule, LoopSnapshot, LoopTriggerKind,
    LoopWatchdogFinding, UpdateLoopSchedulePolicyInput,
};
use serde_json::Value;

#[tauri::command]
pub async fn list_loop_schedules(
    session_id: String,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<Vec<LoopSchedule>, CmdError> {
    app_state
        .session_db
        .list_loop_schedules_for_session_with_cron(&app_state.cron_db, &session_id, 100)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn list_loop_watchdog_findings(
    session_id: String,
    grace_secs: Option<i64>,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<Vec<LoopWatchdogFinding>, CmdError> {
    app_state
        .session_db
        .list_loop_watchdog_findings(&app_state.cron_db, &session_id, grace_secs.unwrap_or(120))
        .map_err(Into::into)
}

#[tauri::command]
pub async fn get_loop_schedule(
    loop_id: String,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<Option<LoopSnapshot>, CmdError> {
    app_state
        .session_db
        .loop_snapshot_with_cron(&app_state.cron_db, &loop_id, 100)
        .map_err(Into::into)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn create_loop_schedule(
    session_id: String,
    trigger_kind: String,
    trigger_spec: Value,
    execution_strategy: Option<String>,
    prompt: Option<String>,
    goal_id: Option<String>,
    goal_criterion_id: Option<String>,
    max_runs: Option<i64>,
    max_runtime_secs: Option<i64>,
    token_budget: Option<i64>,
    cost_budget_micros: Option<i64>,
    max_no_progress_runs: Option<i64>,
    max_failures: Option<i64>,
    backoff_secs: Option<i64>,
    agent_id: Option<String>,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<LoopSchedule, CmdError> {
    let kind = LoopTriggerKind::from_str(&trigger_kind)
        .ok_or_else(|| CmdError::msg(format!("Invalid loop trigger kind: {trigger_kind}")))?;
    let strategy = execution_strategy
        .as_deref()
        .map(|value| {
            LoopExecutionStrategy::from_str(value)
                .ok_or_else(|| CmdError::msg(format!("Invalid loop execution strategy: {value}")))
        })
        .transpose()?
        .unwrap_or_default();
    app_state
        .session_db
        .create_loop_schedule(
            &app_state.cron_db,
            CreateLoopScheduleInput {
                session_id,
                goal_id,
                goal_criterion_id,
                prompt: prompt.unwrap_or_default(),
                trigger_kind: kind,
                trigger_spec,
                execution_strategy: strategy,
                max_runs,
                max_runtime_secs,
                token_budget,
                cost_budget_micros,
                max_no_progress_runs,
                max_failures,
                backoff_secs,
                agent_id,
            },
        )
        .map_err(Into::into)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn update_loop_schedule_policy(
    loop_id: String,
    max_runs: Option<i64>,
    max_runtime_secs: Option<i64>,
    token_budget: Option<i64>,
    max_no_progress_runs: Option<i64>,
    max_failures: Option<i64>,
    backoff_secs: Option<i64>,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<LoopSchedule, CmdError> {
    app_state
        .session_db
        .update_loop_schedule_policy(
            &app_state.cron_db,
            UpdateLoopSchedulePolicyInput {
                loop_id,
                max_runs,
                max_runtime_secs,
                token_budget,
                max_no_progress_runs,
                max_failures,
                backoff_secs,
            },
        )
        .map_err(Into::into)
}

#[tauri::command]
pub async fn run_loop_schedule_now(
    loop_id: String,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<(), CmdError> {
    ha_core::loop_control::spawn_loop_schedule_run_now(
        &app_state.cron_db,
        &app_state.session_db,
        &loop_id,
    )
    .map_err(Into::into)
}

#[tauri::command]
pub async fn pause_loop_schedule(
    loop_id: String,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<LoopSchedule, CmdError> {
    app_state
        .session_db
        .pause_loop_schedule(&app_state.cron_db, &loop_id)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn resume_loop_schedule(
    loop_id: String,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<LoopSchedule, CmdError> {
    app_state
        .session_db
        .resume_loop_schedule(&app_state.cron_db, &loop_id)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn stop_loop_schedule(
    loop_id: String,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<LoopSchedule, CmdError> {
    app_state
        .session_db
        .stop_loop_schedule(&app_state.cron_db, &loop_id)
        .map_err(Into::into)
}
