use axum::extract::Path;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::routes::helpers::session_db;

/// `GET /api/sessions/{session_id}/coding-loop-mode`
pub async fn get_coding_loop_mode(Path(session_id): Path<String>) -> Result<Json<Value>, AppError> {
    let mode = session_db()?
        .get_session_coding_loop_mode(&session_id)?
        .unwrap_or_default();
    Ok(Json(json!({ "mode": mode.as_str() })))
}

#[derive(Debug, Deserialize)]
pub struct SetCodingLoopModeBody {
    pub mode: String,
}

/// `POST /api/sessions/{session_id}/coding-loop-mode`
pub async fn set_coding_loop_mode(
    Path(session_id): Path<String>,
    Json(body): Json<SetCodingLoopModeBody>,
) -> Result<Json<Value>, AppError> {
    let mode = ha_core::coding_loop::CodingLoopMode::from_str(&body.mode)
        .ok_or_else(|| AppError::bad_request("Invalid coding loop mode"))?;
    session_db()?.update_session_coding_loop_mode(&session_id, mode)?;
    Ok(Json(json!({ "mode": mode.as_str() })))
}
