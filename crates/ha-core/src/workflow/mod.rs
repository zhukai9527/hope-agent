//! Durable workflow run store for Phase 2 script-first coding workflows.
//!
//! Runtime execution uses these APIs instead of inventing a parallel
//! run/op/event store.

pub(crate) mod db;
pub(crate) mod events;
pub mod preview;
pub mod runtime;
pub mod types;

pub(crate) use db::ensure_tables;
pub use preview::{
    preview_workflow_run, WorkflowPermissionPreview, WorkflowPermissionPreviewCall,
    WorkflowPermissionPreviewSummary,
};
pub use runtime::{
    recover_pending_workflow_runs, run_workflow_script, run_workflow_script_async,
    spawn_startup_recovery_if_primary, WorkflowRecoveryReport, WorkflowRuntimeResult,
};
pub use types::{
    CreateWorkflowRunInput, StartedOpRecoveryAction, UpsertWorkflowOpInput, WorkflowEffectClass,
    WorkflowEvent, WorkflowOp, WorkflowOpState, WorkflowRun, WorkflowRunSnapshot, WorkflowRunState,
};

#[cfg(test)]
mod tests;
