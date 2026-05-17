//! HTTP routes for macOS control readiness.
//!
//! Server/headless mode has no authorized desktop bridge, so these routes
//! intentionally return the same JSON shape with `supported=false`.

use axum::Json;

use crate::error::AppError;

pub async fn status() -> Result<Json<ha_core::mac_control::MacControlStatus>, AppError> {
    Ok(Json(ha_core::mac_control::status().await))
}

pub async fn permissions(
) -> Result<Json<ha_core::mac_control::MacControlPermissionsResponse>, AppError> {
    Ok(Json(ha_core::mac_control::permissions().await))
}
