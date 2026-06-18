use crate::async_jobs::JobManager;
use crate::context_compact::{JobLedgerItem, RuntimeLedgerSnapshot, SubagentLedgerItem};
use crate::subagent::SubagentStatus;

pub(crate) fn build_runtime_ledger_snapshot(session_id: &str) -> RuntimeLedgerSnapshot {
    let mut snapshot = RuntimeLedgerSnapshot::default();

    match JobManager::list_session_snapshots(session_id) {
        Ok(jobs) => {
            snapshot.background_jobs = jobs
                .into_iter()
                .filter(|job| !job.status.is_terminal())
                .map(|job| {
                    let group_progress = job.child_count.map(|total| {
                        let terminal = job.children_terminal.unwrap_or(0);
                        format!("{}/{}", terminal, total)
                    });
                    JobLedgerItem {
                        job_id: job.job_id,
                        kind: job.kind.as_str().to_string(),
                        status: job.status.as_str().to_string(),
                        label: (!job.label.is_empty()).then_some(job.label),
                        tool: Some(job.tool),
                        group_progress,
                    }
                })
                .collect();
        }
        Err(e) => snapshot
            .warnings
            .push(format!("background_jobs_snapshot_failed: {}", e)),
    }

    if let Some(db) = crate::globals::get_session_db() {
        match db.list_subagent_runs(session_id) {
            Ok(runs) => {
                snapshot.subagents = runs
                    .into_iter()
                    .filter(|run| {
                        matches!(
                            run.status,
                            SubagentStatus::Queued
                                | SubagentStatus::Spawning
                                | SubagentStatus::Running
                        )
                    })
                    .map(|run| SubagentLedgerItem {
                        run_id: run.run_id,
                        status: run.status.as_str().to_string(),
                        child_agent_id: run.child_agent_id,
                        child_session_id: run.child_session_id,
                        task_preview: crate::truncate_utf8(&run.task, 160).to_string(),
                    })
                    .collect();
            }
            Err(e) => snapshot
                .warnings
                .push(format!("subagent_snapshot_failed: {}", e)),
        }
    } else {
        snapshot
            .warnings
            .push("session_db_unavailable_for_subagent_snapshot".to_string());
    }

    snapshot
}
