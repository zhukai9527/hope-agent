use crate::commands::CmdError;
use ha_core::domain_workflow::{
    DomainEvidenceItem, DomainWorkflowDraft, DomainWorkflowTemplate, ListDomainEvidenceInput,
    ListDomainWorkflowTemplatesInput, PreviewDomainWorkflowInput, RecordDomainEvidenceInput,
    SaveDomainWorkflowTemplateInput,
};

#[tauri::command]
pub async fn list_domain_workflow_templates(
    input: ListDomainWorkflowTemplatesInput,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<Vec<DomainWorkflowTemplate>, CmdError> {
    app_state
        .session_db
        .list_domain_workflow_templates(input)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn save_domain_workflow_template(
    input: SaveDomainWorkflowTemplateInput,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<DomainWorkflowTemplate, CmdError> {
    app_state
        .session_db
        .save_domain_workflow_template(input)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn preview_domain_workflow(
    input: PreviewDomainWorkflowInput,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<DomainWorkflowDraft, CmdError> {
    app_state
        .session_db
        .preview_domain_workflow(input)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn record_domain_evidence(
    input: RecordDomainEvidenceInput,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<DomainEvidenceItem, CmdError> {
    app_state
        .session_db
        .record_domain_evidence(input)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn list_domain_evidence(
    input: ListDomainEvidenceInput,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<Vec<DomainEvidenceItem>, CmdError> {
    app_state
        .session_db
        .list_domain_evidence(input)
        .map_err(Into::into)
}
