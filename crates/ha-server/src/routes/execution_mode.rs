use axum::extract::Path;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::routes::helpers::session_db;

/// `GET /api/sessions/{session_id}/execution-mode`
pub async fn get_execution_mode(Path(session_id): Path<String>) -> Result<Json<Value>, AppError> {
    let mode = session_db()?
        .get_session_execution_mode(&session_id)?
        .unwrap_or_default();
    Ok(Json(json!({ "mode": mode.as_str() })))
}

#[derive(Debug, Deserialize)]
pub struct SetExecutionModeBody {
    pub mode: String,
}

/// `POST /api/sessions/{session_id}/execution-mode`
pub async fn set_execution_mode(
    Path(session_id): Path<String>,
    Json(body): Json<SetExecutionModeBody>,
) -> Result<Json<Value>, AppError> {
    let mode = ha_core::execution_mode::ExecutionMode::from_str(&body.mode)
        .ok_or_else(|| AppError::bad_request("Invalid execution mode"))?;
    session_db()?.update_session_execution_mode(&session_id, mode)?;
    Ok(Json(json!({ "mode": mode.as_str() })))
}
