use crate::commands::CmdError;
use ha_core::coding_improvement::{
    ApplyCodingImprovementProposalResult, CodingBenchmarkBacklogItem,
    CodingBenchmarkBacklogListInput, CodingBenchmarkBacklogMaterializeInput,
    CodingBenchmarkBacklogMaterializeResult, CodingBenchmarkBacklogStatusInput,
    CodingBenchmarkCampaign, CodingBenchmarkCampaignCreateInput, CodingBenchmarkCampaignListInput,
    CodingBenchmarkCampaignRunInput, CodingBenchmarkCenterInput, CodingBenchmarkCenterReport,
    CodingBenchmarkComparisonInput, CodingBenchmarkCorpusHealthInput,
    CodingBenchmarkCorpusHealthReport, CodingBenchmarkLeaderboardInput,
    CodingBenchmarkLeaderboardReport, CodingBenchmarkReport, CodingBenchmarkReportGenerateInput,
    CodingBenchmarkReportListInput, CodingBenchmarkReportMarkInput, CodingBenchmarkTaskPack,
    CodingBenchmarkTaskPackImportInput, CodingBenchmarkTaskPackListInput,
    CodingBenchmarkTaskPackStatusInput, CodingBenchmarkTaskPackValidateInput,
    CodingBenchmarkTaskPackValidationReport, CodingContinuousBenchmarkGateInput,
    CodingContinuousBenchmarkGateReport, CodingEvalReleaseGateInput, CodingEvalReleaseGateReport,
    CodingEvalRunRecord, CodingImprovementActionPlan, CodingImprovementPromotionPlan,
    CodingImprovementProposal, CodingLearningGeneralizationInput,
    CodingLearningGeneralizationReport, CodingTrendReport, DistillCodingImprovementResult,
    GenerateCodingImprovementProposalsInput, GenerateCodingImprovementProposalsResult,
    PromoteCodingImprovementProposalResult, RecordCodingEvalRunInput,
};

#[tauri::command]
pub async fn get_coding_trend_report(
    session_id: String,
    window_days: Option<u32>,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<CodingTrendReport, CmdError> {
    app_state
        .session_db
        .coding_trend_report(&session_id, window_days)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn list_coding_improvement_proposals(
    session_id: String,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<Vec<CodingImprovementProposal>, CmdError> {
    app_state
        .session_db
        .list_coding_improvement_proposals(&session_id)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn generate_coding_improvement_proposals(
    session_id: String,
    window_days: Option<u32>,
    source_type: Option<String>,
    source_id: Option<String>,
    proposal_kinds: Option<Vec<String>>,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<GenerateCodingImprovementProposalsResult, CmdError> {
    app_state
        .session_db
        .generate_coding_improvement_proposals_with_input(
            &session_id,
            GenerateCodingImprovementProposalsInput {
                window_days,
                source_type,
                source_id,
                proposal_kinds: proposal_kinds.unwrap_or_default(),
            },
        )
        .map_err(Into::into)
}

#[tauri::command]
pub async fn distill_coding_improvement_proposals(
    session_id: String,
    window_days: Option<u32>,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<DistillCodingImprovementResult, CmdError> {
    app_state
        .session_db
        .distill_coding_improvement_proposals(&session_id, window_days)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn update_coding_improvement_proposal_status(
    proposal_id: String,
    status: String,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<CodingImprovementProposal, CmdError> {
    app_state
        .session_db
        .update_coding_improvement_proposal_status(&proposal_id, &status)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn preview_coding_improvement_proposal_action(
    proposal_id: String,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<CodingImprovementActionPlan, CmdError> {
    app_state
        .session_db
        .preview_coding_improvement_proposal_action(&proposal_id)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn apply_coding_improvement_proposal(
    proposal_id: String,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<ApplyCodingImprovementProposalResult, CmdError> {
    app_state
        .session_db
        .apply_coding_improvement_proposal(&proposal_id)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn preview_coding_improvement_proposal_promotion(
    proposal_id: String,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<CodingImprovementPromotionPlan, CmdError> {
    app_state
        .session_db
        .preview_coding_improvement_proposal_promotion(&proposal_id)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn promote_coding_improvement_proposal(
    proposal_id: String,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<PromoteCodingImprovementProposalResult, CmdError> {
    app_state
        .session_db
        .promote_coding_improvement_proposal(&proposal_id)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn record_coding_eval_run(
    input: RecordCodingEvalRunInput,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<CodingEvalRunRecord, CmdError> {
    app_state
        .session_db
        .record_coding_eval_run(input)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn evaluate_coding_eval_release_gate(
    input: CodingEvalReleaseGateInput,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<CodingEvalReleaseGateReport, CmdError> {
    app_state
        .session_db
        .evaluate_coding_eval_release_gate(input)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn evaluate_coding_learning_generalization(
    input: CodingLearningGeneralizationInput,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<CodingLearningGeneralizationReport, CmdError> {
    app_state
        .session_db
        .evaluate_coding_learning_generalization(input)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn get_coding_benchmark_center(
    input: CodingBenchmarkCenterInput,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<CodingBenchmarkCenterReport, CmdError> {
    app_state
        .session_db
        .get_coding_benchmark_center(input)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn create_coding_benchmark_campaign(
    input: CodingBenchmarkCampaignCreateInput,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<CodingBenchmarkCampaign, CmdError> {
    let run_now = input.run_now;
    let providers = input.gold_task_input.providers.clone();
    let campaign = app_state
        .session_db
        .create_coding_benchmark_campaign(input)
        .map_err(CmdError::from)?;
    if run_now {
        let db = app_state.session_db.clone();
        let campaign_id = campaign.id.clone();
        tokio::spawn(async move {
            let input = CodingBenchmarkCampaignRunInput {
                campaign_id,
                providers,
                retry_failed_only: false,
            };
            let _ = ha_core::coding_eval::run_benchmark_campaign(db, input).await;
        });
    }
    Ok(campaign)
}

#[tauri::command]
pub async fn list_coding_benchmark_campaigns(
    input: CodingBenchmarkCampaignListInput,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<Vec<CodingBenchmarkCampaign>, CmdError> {
    app_state
        .session_db
        .list_coding_benchmark_campaigns(input)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn get_coding_benchmark_campaign(
    campaign_id: String,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<Option<CodingBenchmarkCampaign>, CmdError> {
    app_state
        .session_db
        .get_coding_benchmark_campaign(&campaign_id)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn cancel_coding_benchmark_campaign(
    campaign_id: String,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<Option<CodingBenchmarkCampaign>, CmdError> {
    app_state
        .session_db
        .cancel_coding_benchmark_campaign(&campaign_id)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn run_coding_benchmark_campaign(
    input: CodingBenchmarkCampaignRunInput,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<Option<CodingBenchmarkCampaign>, CmdError> {
    let db = app_state.session_db.clone();
    let campaign_id = input.campaign_id.clone();
    tokio::spawn(async move {
        let _ = ha_core::coding_eval::run_benchmark_campaign(db, input).await;
    });
    app_state
        .session_db
        .get_coding_benchmark_campaign(&campaign_id)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn get_benchmark_leaderboard(
    input: CodingBenchmarkLeaderboardInput,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<CodingBenchmarkLeaderboardReport, CmdError> {
    app_state
        .session_db
        .get_benchmark_leaderboard(input)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn compare_benchmark_models(
    input: CodingBenchmarkComparisonInput,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<CodingBenchmarkLeaderboardReport, CmdError> {
    app_state
        .session_db
        .compare_benchmark_models(input)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn import_benchmark_task_pack(
    input: CodingBenchmarkTaskPackImportInput,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<CodingBenchmarkTaskPack, CmdError> {
    app_state
        .session_db
        .import_benchmark_task_pack(input)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn list_benchmark_task_packs(
    input: CodingBenchmarkTaskPackListInput,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<Vec<CodingBenchmarkTaskPack>, CmdError> {
    app_state
        .session_db
        .list_benchmark_task_packs(input)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn get_benchmark_task_pack(
    pack_id: String,
    version: String,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<Option<CodingBenchmarkTaskPack>, CmdError> {
    app_state
        .session_db
        .get_benchmark_task_pack(&pack_id, &version)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn update_benchmark_task_pack_status(
    input: CodingBenchmarkTaskPackStatusInput,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<CodingBenchmarkTaskPack, CmdError> {
    app_state
        .session_db
        .update_benchmark_task_pack_status(input)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn validate_benchmark_task_pack(
    input: CodingBenchmarkTaskPackValidateInput,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<CodingBenchmarkTaskPackValidationReport, CmdError> {
    app_state
        .session_db
        .validate_benchmark_task_pack(input)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn get_benchmark_corpus_health(
    input: CodingBenchmarkCorpusHealthInput,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<CodingBenchmarkCorpusHealthReport, CmdError> {
    app_state
        .session_db
        .get_benchmark_corpus_health(input)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn generate_benchmark_report(
    input: CodingBenchmarkReportGenerateInput,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<CodingBenchmarkReport, CmdError> {
    app_state
        .session_db
        .generate_benchmark_report(input)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn list_benchmark_reports(
    input: CodingBenchmarkReportListInput,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<Vec<CodingBenchmarkReport>, CmdError> {
    app_state
        .session_db
        .list_benchmark_reports(input)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn get_benchmark_report(
    report_id: String,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<Option<CodingBenchmarkReport>, CmdError> {
    app_state
        .session_db
        .get_benchmark_report(&report_id)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn mark_benchmark_report_release_evidence(
    input: CodingBenchmarkReportMarkInput,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<CodingBenchmarkReport, CmdError> {
    app_state
        .session_db
        .mark_benchmark_report_release_evidence(input)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn evaluate_continuous_benchmark_gate(
    input: CodingContinuousBenchmarkGateInput,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<CodingContinuousBenchmarkGateReport, CmdError> {
    app_state
        .session_db
        .evaluate_continuous_benchmark_gate(input)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn materialize_benchmark_backlog(
    input: CodingBenchmarkBacklogMaterializeInput,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<CodingBenchmarkBacklogMaterializeResult, CmdError> {
    app_state
        .session_db
        .materialize_benchmark_backlog(input)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn list_benchmark_backlog(
    input: CodingBenchmarkBacklogListInput,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<Vec<CodingBenchmarkBacklogItem>, CmdError> {
    app_state
        .session_db
        .list_benchmark_backlog(input)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn update_benchmark_backlog_status(
    input: CodingBenchmarkBacklogStatusInput,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<CodingBenchmarkBacklogItem, CmdError> {
    app_state
        .session_db
        .update_benchmark_backlog_status(input)
        .map_err(Into::into)
}
