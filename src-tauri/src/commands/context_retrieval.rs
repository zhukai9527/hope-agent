use crate::commands::CmdError;
use ha_core::context_retrieval::{
    context_retrieval_for_session, ContextRetrievalInput, ContextRetrievalSnapshot,
};

#[tauri::command]
pub async fn get_context_retrieval(
    session_id: String,
    query: Option<String>,
    limit: Option<usize>,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<ContextRetrievalSnapshot, CmdError> {
    context_retrieval_for_session(
        app_state.session_db.clone(),
        session_id,
        ContextRetrievalInput { query, limit },
    )
    .await
    .map_err(Into::into)
}
