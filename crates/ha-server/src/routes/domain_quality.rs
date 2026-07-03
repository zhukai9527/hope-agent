use axum::{extract::Path, Json};
use ha_core::domain_quality::{DomainQualityRun, DomainQualityRunSnapshot, RunDomainQualityInput};
use serde::Deserialize;

use crate::error::AppError;
use crate::routes::helpers::session_db;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunDomainQualityBody {
    pub input: RunDomainQualityInput,
}

pub async fn list_domain_quality_runs(
    Path(session_id): Path<String>,
) -> Result<Json<Vec<DomainQualityRun>>, AppError> {
    session_db()?
        .list_domain_quality_runs_for_session(&session_id, 100)
        .map(Json)
        .map_err(|e| AppError::bad_request(e.to_string()))
}

pub async fn get_domain_quality_run(
    Path(run_id): Path<String>,
) -> Result<Json<Option<DomainQualityRunSnapshot>>, AppError> {
    session_db()?
        .domain_quality_run_snapshot(&run_id, 100)
        .map(Json)
        .map_err(|e| AppError::bad_request(e.to_string()))
}

pub async fn run_domain_quality(
    Json(body): Json<RunDomainQualityBody>,
) -> Result<Json<DomainQualityRunSnapshot>, AppError> {
    session_db()?
        .run_domain_quality_for_session(body.input)
        .map(Json)
        .map_err(|e| AppError::bad_request(e.to_string()))
}
