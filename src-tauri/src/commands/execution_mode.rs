use crate::commands::CmdError;
use serde_json::{json, Value};

#[tauri::command]
pub async fn get_execution_mode(
    session_id: String,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<Value, CmdError> {
    let mode = app_state
        .session_db
        .get_session_execution_mode(&session_id)?
        .unwrap_or_default();
    Ok(json!({ "mode": mode.as_str() }))
}

#[tauri::command]
pub async fn set_execution_mode(
    session_id: String,
    mode: String,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<Value, CmdError> {
    let parsed = ha_core::execution_mode::ExecutionMode::from_str(&mode)
        .ok_or_else(|| CmdError::msg("Invalid execution mode"))?;
    app_state
        .session_db
        .update_session_execution_mode(&session_id, parsed)?;
    Ok(json!({ "mode": parsed.as_str() }))
}
