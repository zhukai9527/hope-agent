use axum::extract::{Path, Query};
use axum::Json;
use ha_core::coding_improvement::{
    ApplyCodingImprovementProposalResult, CodingBenchmarkCampaign,
    CodingBenchmarkCampaignCreateInput, CodingBenchmarkCampaignListInput,
    CodingBenchmarkCampaignRunInput, CodingBenchmarkCenterInput, CodingBenchmarkCenterReport,
    CodingBenchmarkComparisonInput, CodingBenchmarkLeaderboardInput,
    CodingBenchmarkLeaderboardReport, CodingEvalReleaseGateInput, CodingEvalReleaseGateReport,
    CodingEvalRunRecord, CodingImprovementActionPlan, CodingImprovementPromotionPlan,
    CodingImprovementProposal, CodingLearningGeneralizationInput,
    CodingLearningGeneralizationReport, CodingTrendReport, DistillCodingImprovementResult,
    GenerateCodingImprovementProposalsResult, PromoteCodingImprovementProposalResult,
    RecordCodingEvalRunInput,
};
use serde::Deserialize;

use crate::error::AppError;
use crate::routes::helpers::session_db;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrendQuery {
    pub window_days: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateProposalsBody {
    pub window_days: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProposalStatusBody {
    pub status: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordEvalRunBody {
    pub input: RecordCodingEvalRunInput,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseGateBody {
    pub input: CodingEvalReleaseGateInput,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningGeneralizationBody {
    pub input: CodingLearningGeneralizationInput,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkCenterBody {
    pub input: CodingBenchmarkCenterInput,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkCampaignCreateBody {
    pub input: CodingBenchmarkCampaignCreateInput,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkCampaignListBody {
    pub input: CodingBenchmarkCampaignListInput,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkCampaignRunBody {
    pub input: CodingBenchmarkCampaignRunInput,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkLeaderboardBody {
    pub input: CodingBenchmarkLeaderboardInput,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkComparisonBody {
    pub input: CodingBenchmarkComparisonInput,
}

pub async fn get_coding_trend_report(
    Path(session_id): Path<String>,
    Query(query): Query<TrendQuery>,
) -> Result<Json<CodingTrendReport>, AppError> {
    Ok(Json(
        session_db()?.coding_trend_report(&session_id, query.window_days)?,
    ))
}

pub async fn list_coding_improvement_proposals(
    Path(session_id): Path<String>,
) -> Result<Json<Vec<CodingImprovementProposal>>, AppError> {
    Ok(Json(
        session_db()?.list_coding_improvement_proposals(&session_id)?,
    ))
}

pub async fn generate_coding_improvement_proposals(
    Path(session_id): Path<String>,
    Json(body): Json<GenerateProposalsBody>,
) -> Result<Json<GenerateCodingImprovementProposalsResult>, AppError> {
    Ok(Json(session_db()?.generate_coding_improvement_proposals(
        &session_id,
        body.window_days,
    )?))
}

pub async fn distill_coding_improvement_proposals(
    Path(session_id): Path<String>,
    Json(body): Json<GenerateProposalsBody>,
) -> Result<Json<DistillCodingImprovementResult>, AppError> {
    Ok(Json(session_db()?.distill_coding_improvement_proposals(
        &session_id,
        body.window_days,
    )?))
}

pub async fn update_coding_improvement_proposal_status(
    Path(proposal_id): Path<String>,
    Json(body): Json<UpdateProposalStatusBody>,
) -> Result<Json<CodingImprovementProposal>, AppError> {
    session_db()?
        .update_coding_improvement_proposal_status(&proposal_id, &body.status)
        .map(Json)
        .map_err(|e| AppError::bad_request(e.to_string()))
}

pub async fn preview_coding_improvement_proposal_action(
    Path(proposal_id): Path<String>,
) -> Result<Json<CodingImprovementActionPlan>, AppError> {
    session_db()?
        .preview_coding_improvement_proposal_action(&proposal_id)
        .map(Json)
        .map_err(|e| AppError::bad_request(e.to_string()))
}

pub async fn apply_coding_improvement_proposal(
    Path(proposal_id): Path<String>,
) -> Result<Json<ApplyCodingImprovementProposalResult>, AppError> {
    session_db()?
        .apply_coding_improvement_proposal(&proposal_id)
        .map(Json)
        .map_err(|e| AppError::bad_request(e.to_string()))
}

pub async fn preview_coding_improvement_proposal_promotion(
    Path(proposal_id): Path<String>,
) -> Result<Json<CodingImprovementPromotionPlan>, AppError> {
    session_db()?
        .preview_coding_improvement_proposal_promotion(&proposal_id)
        .map(Json)
        .map_err(|e| AppError::bad_request(e.to_string()))
}

pub async fn promote_coding_improvement_proposal(
    Path(proposal_id): Path<String>,
) -> Result<Json<PromoteCodingImprovementProposalResult>, AppError> {
    session_db()?
        .promote_coding_improvement_proposal(&proposal_id)
        .map(Json)
        .map_err(|e| AppError::bad_request(e.to_string()))
}

pub async fn record_coding_eval_run(
    Json(body): Json<RecordEvalRunBody>,
) -> Result<Json<CodingEvalRunRecord>, AppError> {
    session_db()?
        .record_coding_eval_run(body.input)
        .map(Json)
        .map_err(|e| AppError::bad_request(e.to_string()))
}

pub async fn evaluate_coding_eval_release_gate(
    Json(body): Json<ReleaseGateBody>,
) -> Result<Json<CodingEvalReleaseGateReport>, AppError> {
    session_db()?
        .evaluate_coding_eval_release_gate(body.input)
        .map(Json)
        .map_err(|e| AppError::bad_request(e.to_string()))
}

pub async fn evaluate_coding_learning_generalization(
    Json(body): Json<LearningGeneralizationBody>,
) -> Result<Json<CodingLearningGeneralizationReport>, AppError> {
    session_db()?
        .evaluate_coding_learning_generalization(body.input)
        .map(Json)
        .map_err(|e| AppError::bad_request(e.to_string()))
}

pub async fn get_coding_benchmark_center(
    Json(body): Json<BenchmarkCenterBody>,
) -> Result<Json<CodingBenchmarkCenterReport>, AppError> {
    session_db()?
        .get_coding_benchmark_center(body.input)
        .map(Json)
        .map_err(|e| AppError::bad_request(e.to_string()))
}

pub async fn create_coding_benchmark_campaign(
    Json(body): Json<BenchmarkCampaignCreateBody>,
) -> Result<Json<CodingBenchmarkCampaign>, AppError> {
    let db = session_db()?.clone();
    let run_now = body.input.run_now;
    let providers = body.input.gold_task_input.providers.clone();
    let campaign = db
        .create_coding_benchmark_campaign(body.input)
        .map_err(|e| AppError::bad_request(e.to_string()))?;
    if run_now {
        let run_db = db.clone();
        let campaign_id = campaign.id.clone();
        tokio::spawn(async move {
            let input = CodingBenchmarkCampaignRunInput {
                campaign_id,
                providers,
                retry_failed_only: false,
            };
            let _ = ha_core::coding_eval::run_benchmark_campaign(run_db, input).await;
        });
    }
    Ok(Json(campaign))
}

pub async fn list_coding_benchmark_campaigns(
    Json(body): Json<BenchmarkCampaignListBody>,
) -> Result<Json<Vec<CodingBenchmarkCampaign>>, AppError> {
    session_db()?
        .list_coding_benchmark_campaigns(body.input)
        .map(Json)
        .map_err(|e| AppError::bad_request(e.to_string()))
}

pub async fn get_coding_benchmark_campaign(
    Path(campaign_id): Path<String>,
) -> Result<Json<Option<CodingBenchmarkCampaign>>, AppError> {
    session_db()?
        .get_coding_benchmark_campaign(&campaign_id)
        .map(Json)
        .map_err(|e| AppError::bad_request(e.to_string()))
}

pub async fn cancel_coding_benchmark_campaign(
    Path(campaign_id): Path<String>,
) -> Result<Json<Option<CodingBenchmarkCampaign>>, AppError> {
    session_db()?
        .cancel_coding_benchmark_campaign(&campaign_id)
        .map(Json)
        .map_err(|e| AppError::bad_request(e.to_string()))
}

pub async fn run_coding_benchmark_campaign(
    Json(body): Json<BenchmarkCampaignRunBody>,
) -> Result<Json<Option<CodingBenchmarkCampaign>>, AppError> {
    let db = session_db()?.clone();
    let campaign_id = body.input.campaign_id.clone();
    tokio::spawn(async move {
        let _ = ha_core::coding_eval::run_benchmark_campaign(db, body.input).await;
    });
    session_db()?
        .get_coding_benchmark_campaign(&campaign_id)
        .map(Json)
        .map_err(|e| AppError::bad_request(e.to_string()))
}

pub async fn get_benchmark_leaderboard(
    Json(body): Json<BenchmarkLeaderboardBody>,
) -> Result<Json<CodingBenchmarkLeaderboardReport>, AppError> {
    session_db()?
        .get_benchmark_leaderboard(body.input)
        .map(Json)
        .map_err(|e| AppError::bad_request(e.to_string()))
}

pub async fn compare_benchmark_models(
    Json(body): Json<BenchmarkComparisonBody>,
) -> Result<Json<CodingBenchmarkLeaderboardReport>, AppError> {
    session_db()?
        .compare_benchmark_models(body.input)
        .map(Json)
        .map_err(|e| AppError::bad_request(e.to_string()))
}
