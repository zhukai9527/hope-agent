//! Coding trend report and improvement-loop proposal queue.
//!
//! Phase 3.11 turns the durable coding control-plane traces (Goal, Workflow,
//! Review, Verification, Repair Loop, and eval records) into a deterministic
//! trend report plus draft-only improvement proposals. This module never
//! modifies project guidance, skills, or eval fixtures directly.

use anyhow::{anyhow, bail, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

use crate::review::{ReviewFindingStatus, ReviewSeverity};
use crate::session::SessionDB;
use crate::util::now_rfc3339;
use crate::verification::VerificationStepState;
use crate::workflow::WorkflowRunState;

const DEFAULT_WINDOW_DAYS: u32 = 30;
const MAX_WINDOW_DAYS: u32 = 180;
const MAX_SCOPE_SESSIONS: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingTrendReport {
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub scope: String,
    pub window_days: u32,
    pub generated_at: String,
    pub overview: CodingTrendOverview,
    pub eval: CodingEvalTrend,
    pub review: CodingReviewTrend,
    pub verification: CodingVerificationTrend,
    pub repair_loop: CodingRepairLoopTrend,
    pub failures: Vec<CodingFailureBucket>,
    pub recent_runs: Vec<CodingRunSummary>,
    pub proposals: Vec<CodingImprovementProposal>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingTrendOverview {
    pub sessions: usize,
    pub goals: usize,
    pub completed_goals: usize,
    pub blocked_goals: usize,
    pub workflow_runs: usize,
    pub completed_workflows: usize,
    pub blocked_workflows: usize,
    pub failed_workflows: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal_completion_rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow_completion_rate: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingEvalTrend {
    pub runs: usize,
    pub passed: usize,
    pub failed: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success_rate: Option<f64>,
    pub backlog_candidates: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingReviewTrend {
    pub runs: usize,
    pub findings: usize,
    pub blocking_findings: usize,
    pub resolved_findings: usize,
    pub false_positive_findings: usize,
    pub by_category: Vec<CodingMetricBucket>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingVerificationTrend {
    pub runs: usize,
    pub steps: usize,
    pub passed_steps: usize,
    pub failed_steps: usize,
    pub timed_out_steps: usize,
    pub planned_only_runs: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executed_success_rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommendation_coverage: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingRepairLoopTrend {
    pub runs: usize,
    pub completed: usize,
    pub blocked: usize,
    pub exhausted: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success_rate: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingMetricBucket {
    pub key: String,
    pub label: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingFailureBucket {
    pub category: String,
    pub label: String,
    pub count: usize,
    pub severity: String,
    #[serde(default)]
    pub examples: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingRunSummary {
    pub run_id: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal_id: Option<String>,
    pub kind: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_category: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingImprovementProposal {
    pub id: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub kind: String,
    pub status: String,
    pub source_type: String,
    pub source_id: String,
    pub title: String,
    pub body: String,
    pub payload: Value,
    pub fingerprint: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateCodingImprovementProposalsResult {
    pub inserted: usize,
    pub proposals: Vec<CodingImprovementProposal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordCodingEvalRunInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub suite: String,
    pub name: String,
    pub status: String,
    #[serde(default)]
    pub metrics: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingEvalRunRecord {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub suite: String,
    pub name: String,
    pub status: String,
    pub metrics: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    pub created_at: String,
}

struct ReportScope {
    session_id: String,
    project_id: Option<String>,
    session_ids: Vec<String>,
    window_days: u32,
    since: String,
}

pub(crate) fn ensure_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS coding_eval_runs (
            id TEXT PRIMARY KEY,
            session_id TEXT,
            project_id TEXT,
            suite TEXT NOT NULL,
            name TEXT NOT NULL,
            status TEXT NOT NULL,
            metrics_json TEXT NOT NULL DEFAULT '{}',
            source_type TEXT,
            source_id TEXT,
            created_at TEXT NOT NULL,
            FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_coding_eval_runs_scope
            ON coding_eval_runs(project_id, session_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_coding_eval_runs_status
            ON coding_eval_runs(status, created_at DESC);

        CREATE TABLE IF NOT EXISTS coding_improvement_proposals (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            project_id TEXT,
            kind TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'draft',
            source_type TEXT NOT NULL,
            source_id TEXT NOT NULL,
            title TEXT NOT NULL,
            body TEXT NOT NULL,
            payload_json TEXT NOT NULL DEFAULT '{}',
            fingerprint TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            decided_at TEXT,
            FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE,
            UNIQUE(session_id, fingerprint)
        );

        CREATE INDEX IF NOT EXISTS idx_coding_improvement_session
            ON coding_improvement_proposals(session_id, status, updated_at DESC);
        CREATE INDEX IF NOT EXISTS idx_coding_improvement_project
            ON coding_improvement_proposals(project_id, status, updated_at DESC);",
    )?;
    Ok(())
}

impl SessionDB {
    pub fn coding_trend_report(
        &self,
        session_id: &str,
        window_days: Option<u32>,
    ) -> Result<CodingTrendReport> {
        let scope = self.resolve_coding_report_scope(session_id, window_days)?;
        let mut report = self.build_coding_trend_report(&scope)?;
        report.proposals = self.list_coding_improvement_proposals_for_scope(&scope)?;
        Ok(report)
    }

    pub fn generate_coding_improvement_proposals(
        &self,
        session_id: &str,
        window_days: Option<u32>,
    ) -> Result<GenerateCodingImprovementProposalsResult> {
        let scope = self.resolve_coding_report_scope(session_id, window_days)?;
        let report = self.build_coding_trend_report(&scope)?;
        let candidates = build_proposal_candidates(&report);
        let mut inserted = 0usize;
        for candidate in candidates {
            if self.insert_coding_improvement_proposal(&scope, candidate)? {
                inserted += 1;
            }
        }
        let proposals = self.list_coding_improvement_proposals_for_scope(&scope)?;
        Ok(GenerateCodingImprovementProposalsResult {
            inserted,
            proposals,
        })
    }

    pub fn list_coding_improvement_proposals(
        &self,
        session_id: &str,
    ) -> Result<Vec<CodingImprovementProposal>> {
        let scope = self.resolve_coding_report_scope(session_id, None)?;
        self.list_coding_improvement_proposals_for_scope(&scope)
    }

    pub fn update_coding_improvement_proposal_status(
        &self,
        proposal_id: &str,
        status: &str,
    ) -> Result<CodingImprovementProposal> {
        let status = normalize_proposal_status(status)?;
        let now = now_rfc3339();
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        let changed = conn.execute(
            "UPDATE coding_improvement_proposals
             SET status = ?1, updated_at = ?2, decided_at = CASE WHEN ?1 = 'draft' THEN NULL ELSE ?2 END
             WHERE id = ?3",
            params![status, now, proposal_id],
        )?;
        if changed == 0 {
            bail!("coding improvement proposal not found: {proposal_id}");
        }
        drop(conn);
        self.get_coding_improvement_proposal(proposal_id)?
            .ok_or_else(|| anyhow!("coding improvement proposal vanished after update"))
    }

    pub fn record_coding_eval_run(
        &self,
        input: RecordCodingEvalRunInput,
    ) -> Result<CodingEvalRunRecord> {
        let status = normalize_eval_status(&input.status)?;
        let session_id = input
            .session_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        if let Some(session_id) = session_id.as_deref() {
            let meta = self
                .get_session(session_id)?
                .ok_or_else(|| anyhow!("session not found: {session_id}"))?;
            if meta.incognito {
                bail!("Cannot record coding eval run for incognito session {session_id}");
            }
        }
        let project_id = input
            .project_id
            .or_else(|| {
                session_id
                    .as_deref()
                    .and_then(|sid| self.get_session(sid).ok().flatten())
                    .and_then(|meta| meta.project_id)
            })
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let suite = input.suite.trim();
        let name = input.name.trim();
        if suite.is_empty() || name.is_empty() {
            bail!("coding eval run suite and name must not be empty");
        }
        let id = format!("cer_{}", uuid::Uuid::new_v4().simple());
        let now = now_rfc3339();
        let metrics_json = stable_json(&input.metrics)?;
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        conn.execute(
            "INSERT INTO coding_eval_runs (
                id, session_id, project_id, suite, name, status, metrics_json,
                source_type, source_id, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                id,
                session_id,
                project_id,
                suite,
                name,
                status,
                metrics_json,
                input.source_type,
                input.source_id,
                now
            ],
        )?;
        drop(conn);
        self.get_coding_eval_run(&id)?
            .ok_or_else(|| anyhow!("coding eval run vanished after insert"))
    }

    fn resolve_coding_report_scope(
        &self,
        session_id: &str,
        window_days: Option<u32>,
    ) -> Result<ReportScope> {
        let window_days = window_days
            .unwrap_or(DEFAULT_WINDOW_DAYS)
            .clamp(1, MAX_WINDOW_DAYS);
        let since = chrono::Utc::now()
            .checked_sub_signed(chrono::Duration::days(window_days as i64))
            .unwrap_or_else(chrono::Utc::now)
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let meta = self
            .get_session(session_id)?
            .ok_or_else(|| anyhow!("session not found: {session_id}"))?;
        if meta.incognito {
            bail!("Cannot build durable coding trend report for incognito session {session_id}");
        }
        let session_ids = if let Some(project_id) = meta.project_id.as_deref() {
            let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
            let mut stmt = conn.prepare(
                "SELECT id FROM sessions
                 WHERE project_id = ?1
                   AND incognito = 0
                   AND (updated_at >= ?2 OR id = ?3)
                 ORDER BY updated_at DESC
                 LIMIT ?4",
            )?;
            let rows = stmt.query_map(
                params![project_id, since, session_id, MAX_SCOPE_SESSIONS as i64],
                |row| row.get::<_, String>(0),
            )?;
            collect_rows(rows)?
        } else {
            vec![session_id.to_string()]
        };
        Ok(ReportScope {
            session_id: session_id.to_string(),
            project_id: meta.project_id,
            session_ids,
            window_days,
            since,
        })
    }

    fn build_coding_trend_report(&self, scope: &ReportScope) -> Result<CodingTrendReport> {
        let mut overview = CodingTrendOverview {
            sessions: scope.session_ids.len(),
            ..CodingTrendOverview::default()
        };
        let mut eval = CodingEvalTrend::default();
        let mut review = CodingReviewTrend::default();
        let mut verification = CodingVerificationTrend::default();
        let mut repair_loop = CodingRepairLoopTrend::default();
        let mut failures: BTreeMap<String, CodingFailureBucket> = BTreeMap::new();
        let mut recent_runs = Vec::new();
        let mut review_categories: BTreeMap<String, usize> = BTreeMap::new();

        let eval_runs = self.list_coding_eval_runs_for_scope(scope)?;
        eval.runs = eval_runs.len();
        eval.passed = eval_runs
            .iter()
            .filter(|run| run.status == "passed")
            .count();
        eval.failed = eval_runs
            .iter()
            .filter(|run| run.status == "failed")
            .count();
        eval.success_rate = ratio(eval.passed, eval.passed + eval.failed);

        for session_id in &scope.session_ids {
            let goals = self.list_goal_rows_for_session(session_id, &scope.since)?;
            overview.goals += goals.len();
            for goal in goals {
                match goal.state.as_str() {
                    "completed" => overview.completed_goals += 1,
                    "blocked" => {
                        overview.blocked_goals += 1;
                        add_failure(
                            &mut failures,
                            classify_blocked_reason(goal.blocked_reason.as_deref()),
                            goal.blocked_reason
                                .unwrap_or_else(|| "goal blocked".to_string()),
                            "goal",
                        );
                    }
                    "failed" => add_failure(&mut failures, "goal_failed", "goal failed", "goal"),
                    _ => {}
                }
            }

            for run in self.list_workflow_runs_for_session(session_id, 200)? {
                if run.updated_at < scope.since {
                    continue;
                }
                overview.workflow_runs += 1;
                let events = self.list_workflow_events(&run.id, 500).unwrap_or_default();
                let has_repair_loop = events.iter().any(|event| {
                    event.event_type.starts_with("repair_loop_")
                        || event.event_type == "workflow_block_requested"
                }) || run.script_source.contains("repairLoop");
                if has_repair_loop {
                    repair_loop.runs += 1;
                }
                match run.state {
                    WorkflowRunState::Completed => {
                        overview.completed_workflows += 1;
                        if has_repair_loop {
                            repair_loop.completed += 1;
                        }
                    }
                    WorkflowRunState::Blocked => {
                        overview.blocked_workflows += 1;
                        if has_repair_loop {
                            repair_loop.blocked += 1;
                        }
                        if run.blocked_reason.as_deref() == Some("repair_loop_attempts_exhausted") {
                            repair_loop.exhausted += 1;
                        }
                        add_failure(
                            &mut failures,
                            classify_blocked_reason(run.blocked_reason.as_deref()),
                            run.blocked_reason
                                .clone()
                                .unwrap_or_else(|| "workflow blocked".to_string()),
                            &run.id,
                        );
                    }
                    WorkflowRunState::Failed => {
                        overview.failed_workflows += 1;
                        add_failure(&mut failures, "workflow_failed", "workflow failed", &run.id);
                    }
                    WorkflowRunState::AwaitingApproval => {
                        add_failure(
                            &mut failures,
                            "permission_stall",
                            "workflow awaiting approval",
                            &run.id,
                        );
                    }
                    _ => {}
                }
                if !matches!(run.state, WorkflowRunState::Draft) {
                    recent_runs.push(CodingRunSummary {
                        run_id: run.id.clone(),
                        session_id: run.session_id.clone(),
                        goal_id: run.goal_id.clone(),
                        kind: run.kind.clone(),
                        state: run.state.as_str().to_string(),
                        blocked_reason: run.blocked_reason.clone(),
                        failure_category: if matches!(
                            run.state,
                            WorkflowRunState::Blocked | WorkflowRunState::Failed
                        ) {
                            Some(classify_blocked_reason(run.blocked_reason.as_deref()).to_string())
                        } else {
                            None
                        },
                        updated_at: run.updated_at.clone(),
                    });
                }
            }

            for review_run in self.list_review_runs_for_session(session_id, 200)? {
                if review_run.updated_at < scope.since {
                    continue;
                }
                review.runs += 1;
                let findings = self
                    .list_review_findings_for_run(&review_run.id)
                    .unwrap_or_default();
                review.findings += findings.len();
                for finding in findings {
                    *review_categories
                        .entry(finding.category.clone())
                        .or_default() += 1;
                    if is_blocking_review_finding(&finding.severity, &finding.status) {
                        review.blocking_findings += 1;
                        add_failure(
                            &mut failures,
                            "review_blocker",
                            finding.title.clone(),
                            &finding.id,
                        );
                    }
                    if finding.status == ReviewFindingStatus::Resolved {
                        review.resolved_findings += 1;
                    }
                    if finding.status == ReviewFindingStatus::FalsePositive {
                        review.false_positive_findings += 1;
                    }
                }
            }

            for verification_run in self.list_verification_runs_for_session(session_id, 200)? {
                if verification_run.updated_at < scope.since {
                    continue;
                }
                verification.runs += 1;
                let steps = self
                    .list_verification_steps_for_run(&verification_run.id)
                    .unwrap_or_default();
                if matches!(verification_run.state.as_str(), "planned") {
                    verification.planned_only_runs += 1;
                }
                if steps.is_empty() {
                    add_failure(
                        &mut failures,
                        "verification_selection_gap",
                        "verification plan selected no command",
                        &verification_run.id,
                    );
                }
                verification.steps += steps.len();
                for step in steps {
                    match step.state {
                        VerificationStepState::Passed => verification.passed_steps += 1,
                        VerificationStepState::Failed => {
                            verification.failed_steps += 1;
                            add_failure(
                                &mut failures,
                                "validation_failed",
                                step.title.clone(),
                                &step.id,
                            );
                        }
                        VerificationStepState::TimedOut => {
                            verification.timed_out_steps += 1;
                            add_failure(
                                &mut failures,
                                "validation_failed",
                                format!("{} timed out", step.title),
                                &step.id,
                            );
                        }
                        _ => {}
                    }
                }
            }
        }

        overview.goal_completion_rate = ratio(
            overview.completed_goals,
            overview.completed_goals + overview.blocked_goals,
        );
        overview.workflow_completion_rate = ratio(
            overview.completed_workflows,
            overview.completed_workflows + overview.blocked_workflows + overview.failed_workflows,
        );
        repair_loop.success_rate = ratio(
            repair_loop.completed,
            repair_loop.completed + repair_loop.blocked,
        );
        let executed =
            verification.passed_steps + verification.failed_steps + verification.timed_out_steps;
        verification.executed_success_rate = ratio(verification.passed_steps, executed);
        verification.recommendation_coverage = ratio(
            verification
                .runs
                .saturating_sub(count_zero_step_verification_runs(self, scope)?),
            verification.runs,
        );
        review.by_category = review_categories
            .into_iter()
            .map(|(key, count)| CodingMetricBucket {
                label: failure_label(&key).unwrap_or(&key).to_string(),
                key,
                count,
            })
            .collect();
        eval.backlog_candidates = self.count_eval_candidate_proposals_for_scope(scope)?;
        let mut failures = failures.into_values().collect::<Vec<_>>();
        failures.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then_with(|| a.category.cmp(&b.category))
        });
        recent_runs.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        recent_runs.truncate(12);

        Ok(CodingTrendReport {
            session_id: scope.session_id.clone(),
            project_id: scope.project_id.clone(),
            scope: if scope.project_id.is_some() {
                "project".to_string()
            } else {
                "session".to_string()
            },
            window_days: scope.window_days,
            generated_at: now_rfc3339(),
            overview,
            eval,
            review,
            verification,
            repair_loop,
            failures,
            recent_runs,
            proposals: Vec::new(),
        })
    }

    fn list_goal_rows_for_session(&self, session_id: &str, since: &str) -> Result<Vec<GoalRow>> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        let mut stmt = conn.prepare(
            "SELECT id, state, blocked_reason, updated_at
             FROM goals
             WHERE session_id = ?1 AND updated_at >= ?2
             ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map(params![session_id, since], |row| {
            Ok(GoalRow {
                id: row.get(0)?,
                state: row.get(1)?,
                blocked_reason: row.get(2)?,
                updated_at: row.get(3)?,
            })
        })?;
        collect_rows(rows)
    }

    fn list_coding_eval_runs_for_scope(
        &self,
        scope: &ReportScope,
    ) -> Result<Vec<CodingEvalRunRecord>> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        let mut out = Vec::new();
        if let Some(project_id) = scope.project_id.as_deref() {
            let mut stmt = conn.prepare(
                "SELECT id, session_id, project_id, suite, name, status, metrics_json,
                        source_type, source_id, created_at
                 FROM coding_eval_runs
                 WHERE project_id = ?1 AND created_at >= ?2
                 ORDER BY created_at DESC
                 LIMIT 200",
            )?;
            let rows = stmt.query_map(params![project_id, scope.since], row_to_eval_run)?;
            out.extend(collect_rows(rows)?);
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, session_id, project_id, suite, name, status, metrics_json,
                        source_type, source_id, created_at
                 FROM coding_eval_runs
                 WHERE session_id = ?1 AND created_at >= ?2
                 ORDER BY created_at DESC
                 LIMIT 200",
            )?;
            let rows = stmt.query_map(params![scope.session_id, scope.since], row_to_eval_run)?;
            out.extend(collect_rows(rows)?);
        }
        Ok(out)
    }

    fn get_coding_eval_run(&self, id: &str) -> Result<Option<CodingEvalRunRecord>> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        conn.query_row(
            "SELECT id, session_id, project_id, suite, name, status, metrics_json,
                    source_type, source_id, created_at
             FROM coding_eval_runs
             WHERE id = ?1",
            params![id],
            row_to_eval_run,
        )
        .optional()
        .map_err(Into::into)
    }

    fn get_coding_improvement_proposal(
        &self,
        id: &str,
    ) -> Result<Option<CodingImprovementProposal>> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        conn.query_row(
            "SELECT id, session_id, project_id, kind, status, source_type, source_id,
                    title, body, payload_json, fingerprint, created_at, updated_at, decided_at
             FROM coding_improvement_proposals
             WHERE id = ?1",
            params![id],
            row_to_proposal,
        )
        .optional()
        .map_err(Into::into)
    }

    fn list_coding_improvement_proposals_for_scope(
        &self,
        scope: &ReportScope,
    ) -> Result<Vec<CodingImprovementProposal>> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        if let Some(project_id) = scope.project_id.as_deref() {
            let mut stmt = conn.prepare(
                "SELECT id, session_id, project_id, kind, status, source_type, source_id,
                        title, body, payload_json, fingerprint, created_at, updated_at, decided_at
                 FROM coding_improvement_proposals
                 WHERE project_id = ?1
                 ORDER BY CASE status WHEN 'draft' THEN 0 ELSE 1 END, updated_at DESC
                 LIMIT 100",
            )?;
            let rows = stmt.query_map(params![project_id], row_to_proposal)?;
            collect_rows(rows)
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, session_id, project_id, kind, status, source_type, source_id,
                        title, body, payload_json, fingerprint, created_at, updated_at, decided_at
                 FROM coding_improvement_proposals
                 WHERE session_id = ?1
                 ORDER BY CASE status WHEN 'draft' THEN 0 ELSE 1 END, updated_at DESC
                 LIMIT 100",
            )?;
            let rows = stmt.query_map(params![scope.session_id], row_to_proposal)?;
            collect_rows(rows)
        }
    }

    fn count_eval_candidate_proposals_for_scope(&self, scope: &ReportScope) -> Result<usize> {
        let proposals = self.list_coding_improvement_proposals_for_scope(scope)?;
        Ok(proposals
            .iter()
            .filter(|proposal| proposal.kind == "eval_candidate")
            .count())
    }

    fn insert_coding_improvement_proposal(
        &self,
        scope: &ReportScope,
        candidate: NewProposal,
    ) -> Result<bool> {
        let id = format!("cip_{}", uuid::Uuid::new_v4().simple());
        let now = now_rfc3339();
        let payload_json = stable_json(&candidate.payload)?;
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        let changed = conn.execute(
            "INSERT OR IGNORE INTO coding_improvement_proposals (
                id, session_id, project_id, kind, status, source_type, source_id,
                title, body, payload_json, fingerprint, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, 'draft', ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
            params![
                id,
                scope.session_id,
                scope.project_id,
                candidate.kind,
                candidate.source_type,
                candidate.source_id,
                candidate.title,
                candidate.body,
                payload_json,
                candidate.fingerprint,
                now
            ],
        )?;
        Ok(changed > 0)
    }
}

#[derive(Debug)]
struct GoalRow {
    #[allow(dead_code)]
    id: String,
    state: String,
    blocked_reason: Option<String>,
    #[allow(dead_code)]
    updated_at: String,
}

struct NewProposal {
    kind: String,
    source_type: String,
    source_id: String,
    title: String,
    body: String,
    payload: Value,
    fingerprint: String,
}

fn build_proposal_candidates(report: &CodingTrendReport) -> Vec<NewProposal> {
    let mut out = Vec::new();
    for failure in report.failures.iter().take(3) {
        out.push(NewProposal {
            kind: "eval_candidate".to_string(),
            source_type: "failure_taxonomy".to_string(),
            source_id: failure.category.clone(),
            title: format!("Add eval coverage for {}", failure.label),
            body: format!(
                "{} occurrence(s) in the last {} days. Convert one representative failure into a deterministic eval candidate before changing policy.",
                failure.count, report.window_days
            ),
            payload: json!({
                "proposalType": "eval_candidate",
                "failure": failure,
                "scope": report.scope,
                "projectId": report.project_id,
                "expectedSignals": expected_signals_for_failure(&failure.category),
            }),
            fingerprint: format!("eval:{}:{}", report.scope_key(), failure.category),
        });
    }

    if report.repair_loop.completed > 0 {
        out.push(NewProposal {
            kind: "workflow_template".to_string(),
            source_type: "repair_loop".to_string(),
            source_id: "completed".to_string(),
            title: "Promote successful repair loop shape".to_string(),
            body: "Recent repair loop runs completed successfully. Review whether the validation/review profile mix should become a reusable workflow draft.".to_string(),
            payload: json!({
                "proposalType": "workflow_template",
                "repairLoop": report.repair_loop,
                "recentRuns": report.recent_runs.iter().take(5).collect::<Vec<_>>(),
            }),
            fingerprint: format!("workflow-template:{}:repair-loop", report.scope_key()),
        });
    }

    if report.review.blocking_findings > 0 {
        out.push(NewProposal {
            kind: "guidance_candidate".to_string(),
            source_type: "review".to_string(),
            source_id: "blocking_findings".to_string(),
            title: "Review blocker pattern needs project guidance".to_string(),
            body: "Open P0/P1 review findings are recurring in this scope. Draft project guidance or workflow checkpoints before making this automatic.".to_string(),
            payload: json!({
                "proposalType": "guidance_candidate",
                "review": report.review,
            }),
            fingerprint: format!("guidance:{}:review-blockers", report.scope_key()),
        });
    }

    if report.verification.failed_steps + report.verification.timed_out_steps > 0 {
        out.push(NewProposal {
            kind: "guidance_candidate".to_string(),
            source_type: "verification".to_string(),
            source_id: "failed_steps".to_string(),
            title: "Verification failures need a tighter playbook".to_string(),
            body: "Recent validation failures or timeouts suggest the project may need more specific targeted verification guidance.".to_string(),
            payload: json!({
                "proposalType": "guidance_candidate",
                "verification": report.verification,
            }),
            fingerprint: format!("guidance:{}:verification-failures", report.scope_key()),
        });
    }

    if report.overview.completed_workflows > 0 && report.failures.is_empty() {
        out.push(NewProposal {
            kind: "skill_candidate".to_string(),
            source_type: "workflow".to_string(),
            source_id: "clean_success".to_string(),
            title: "Distil a clean coding workflow skill draft".to_string(),
            body: "Recent coding workflows completed without classified blockers. Review one transcript manually before promoting a reusable skill.".to_string(),
            payload: json!({
                "proposalType": "skill_candidate",
                "overview": report.overview,
                "recentRuns": report.recent_runs.iter().take(5).collect::<Vec<_>>(),
            }),
            fingerprint: format!("skill:{}:clean-workflow", report.scope_key()),
        });
    }
    out
}

trait ReportScopeKey {
    fn scope_key(&self) -> String;
}

impl ReportScopeKey for CodingTrendReport {
    fn scope_key(&self) -> String {
        self.project_id
            .clone()
            .unwrap_or_else(|| self.session_id.clone())
    }
}

fn expected_signals_for_failure(category: &str) -> Vec<&'static str> {
    match category {
        "validation_failed" => vec!["verification_step", "validation_failed", "command_output"],
        "review_blocker" => vec!["review_finding", "blocking_severity", "file_path"],
        "repair_loop_exhausted" => vec!["workflow_blocked", "repair_loop_attempts_exhausted"],
        "no_effective_diff_progress" => vec!["workflow_blocked", "diff_snapshot"],
        "permission_stall" => vec!["approval", "workflow_state"],
        "context_miss" => vec!["context_candidate", "critical_context_recall"],
        _ => vec!["workflow_run", "goal_evidence"],
    }
}

fn add_failure(
    failures: &mut BTreeMap<String, CodingFailureBucket>,
    category: &str,
    example: impl Into<String>,
    source_id: &str,
) {
    let bucket = failures
        .entry(category.to_string())
        .or_insert_with(|| CodingFailureBucket {
            category: category.to_string(),
            label: failure_label(category).unwrap_or(category).to_string(),
            count: 0,
            severity: failure_severity(category).to_string(),
            examples: Vec::new(),
        });
    bucket.count += 1;
    if bucket.examples.len() < 3 {
        let example = example.into();
        bucket.examples.push(if source_id.is_empty() {
            example
        } else {
            format!("{source_id}: {example}")
        });
    }
}

fn classify_blocked_reason(reason: Option<&str>) -> &'static str {
    let Some(reason) = reason.map(str::to_ascii_lowercase) else {
        return "workflow_blocked";
    };
    if reason.contains("repair_loop_attempts_exhausted") {
        "repair_loop_exhausted"
    } else if reason.contains("no_effective_diff") || reason.contains("no_valid_diff") {
        "no_effective_diff_progress"
    } else if reason.contains("approval") || reason.contains("permission") {
        "permission_stall"
    } else if reason.contains("context") || reason.contains("recall") || reason.contains("missing")
    {
        "context_miss"
    } else if reason.contains("validation") || reason.contains("verify") {
        "validation_failed"
    } else {
        "workflow_blocked"
    }
}

fn failure_label(category: &str) -> Option<&'static str> {
    Some(match category {
        "validation_failed" => "Validation failed",
        "review_blocker" => "Review blocker",
        "repair_loop_exhausted" => "Repair loop exhausted",
        "no_effective_diff_progress" => "No effective diff progress",
        "permission_stall" => "Permission stall",
        "context_miss" => "Context miss",
        "verification_selection_gap" => "Verification selection gap",
        "workflow_failed" => "Workflow failed",
        "workflow_blocked" => "Workflow blocked",
        "goal_failed" => "Goal failed",
        "correctness" => "Correctness",
        "security" => "Security",
        "maintainability" => "Maintainability",
        "tests" => "Tests",
        "frontend" => "Frontend",
        "accessibility" => "Accessibility",
        "concurrency" => "Concurrency",
        _ => return None,
    })
}

fn failure_severity(category: &str) -> &'static str {
    match category {
        "validation_failed" | "review_blocker" | "repair_loop_exhausted" | "permission_stall" => {
            "high"
        }
        "no_effective_diff_progress" | "context_miss" | "workflow_failed" => "medium",
        _ => "low",
    }
}

fn is_blocking_review_finding(severity: &ReviewSeverity, status: &ReviewFindingStatus) -> bool {
    matches!(severity, ReviewSeverity::P0 | ReviewSeverity::P1)
        && matches!(status, ReviewFindingStatus::Open)
}

fn normalize_proposal_status(status: &str) -> Result<&'static str> {
    match status.trim() {
        "draft" | "open" => Ok("draft"),
        "accepted" | "approve" | "approved" => Ok("accepted"),
        "rejected" | "dismissed" | "reject" => Ok("rejected"),
        other => bail!("unsupported coding improvement proposal status: {other}"),
    }
}

fn normalize_eval_status(status: &str) -> Result<&'static str> {
    match status.trim() {
        "passed" | "pass" | "ok" => Ok("passed"),
        "failed" | "fail" | "error" => Ok("failed"),
        "blocked" => Ok("blocked"),
        other => bail!("unsupported coding eval status: {other}"),
    }
}

fn ratio(numerator: usize, denominator: usize) -> Option<f64> {
    if denominator == 0 {
        None
    } else {
        Some((numerator as f64 / denominator as f64 * 1000.0).round() / 1000.0)
    }
}

fn stable_json(value: &Value) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

fn collect_rows<T>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>>,
) -> Result<Vec<T>> {
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn row_to_eval_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<CodingEvalRunRecord> {
    let metrics_json: String = row.get(6)?;
    Ok(CodingEvalRunRecord {
        id: row.get(0)?,
        session_id: row.get(1)?,
        project_id: row.get(2)?,
        suite: row.get(3)?,
        name: row.get(4)?,
        status: row.get(5)?,
        metrics: serde_json::from_str(&metrics_json).unwrap_or_else(|_| json!({})),
        source_type: row.get(7)?,
        source_id: row.get(8)?,
        created_at: row.get(9)?,
    })
}

fn row_to_proposal(row: &rusqlite::Row<'_>) -> rusqlite::Result<CodingImprovementProposal> {
    let payload_json: String = row.get(9)?;
    Ok(CodingImprovementProposal {
        id: row.get(0)?,
        session_id: row.get(1)?,
        project_id: row.get(2)?,
        kind: row.get(3)?,
        status: row.get(4)?,
        source_type: row.get(5)?,
        source_id: row.get(6)?,
        title: row.get(7)?,
        body: row.get(8)?,
        payload: serde_json::from_str(&payload_json).unwrap_or_else(|_| json!({})),
        fingerprint: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
        decided_at: row.get(13)?,
    })
}

fn count_zero_step_verification_runs(db: &SessionDB, scope: &ReportScope) -> Result<usize> {
    let mut count = 0usize;
    for session_id in &scope.session_ids {
        for run in db.list_verification_runs_for_session(session_id, 200)? {
            if run.updated_at >= scope.since
                && db
                    .list_verification_steps_for_run(&run.id)
                    .unwrap_or_default()
                    .is_empty()
            {
                count += 1;
            }
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> (tempfile::TempDir, SessionDB) {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = SessionDB::open(&dir.path().join("sessions.db")).expect("session db");
        ensure_channel_conversations_table(&db);
        (dir, db)
    }

    fn ensure_channel_conversations_table(db: &SessionDB) {
        let conn = db.conn.lock().expect("lock connection");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS channel_conversations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                channel_id TEXT NOT NULL,
                account_id TEXT NOT NULL,
                chat_id TEXT NOT NULL,
                thread_id TEXT,
                session_id TEXT NOT NULL,
                sender_id TEXT,
                sender_name TEXT,
                chat_type TEXT NOT NULL DEFAULT 'dm',
                source TEXT NOT NULL DEFAULT 'inbound',
                attached_at TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );",
        )
        .expect("create channel conversations table");
    }

    #[test]
    fn report_records_eval_success_rate() {
        let (_dir, db) = test_db();
        let session = db
            .create_session(crate::agent_loader::DEFAULT_AGENT_ID)
            .unwrap();
        db.record_coding_eval_run(RecordCodingEvalRunInput {
            session_id: Some(session.id.clone()),
            project_id: None,
            suite: "coding_control_plane".to_string(),
            name: "sample_pass".to_string(),
            status: "passed".to_string(),
            metrics: json!({"criticalContextRecall": 1.0}),
            source_type: None,
            source_id: None,
        })
        .unwrap();
        db.record_coding_eval_run(RecordCodingEvalRunInput {
            session_id: Some(session.id.clone()),
            project_id: None,
            suite: "coding_control_plane".to_string(),
            name: "sample_fail".to_string(),
            status: "failed".to_string(),
            metrics: json!({"criticalContextRecall": 0.5}),
            source_type: None,
            source_id: None,
        })
        .unwrap();

        let report = db.coding_trend_report(&session.id, Some(30)).unwrap();
        assert_eq!(report.eval.runs, 2);
        assert_eq!(report.eval.passed, 1);
        assert_eq!(report.eval.failed, 1);
        assert_eq!(report.eval.success_rate, Some(0.5));
    }

    #[test]
    fn proposals_are_draft_only_and_deduped() {
        let (_dir, db) = test_db();
        let session = db
            .create_session(crate::agent_loader::DEFAULT_AGENT_ID)
            .unwrap();
        let goal = db
            .create_goal(crate::goal::CreateGoalInput {
                session_id: session.id.clone(),
                objective: "finish".to_string(),
                completion_criteria: "validated".to_string(),
                budget_token_limit: None,
                budget_time_limit_secs: None,
                budget_turn_limit: None,
            })
            .unwrap();
        db.transition_goal(
            &goal.goal.id,
            crate::goal::GoalState::Blocked,
            Some("context miss"),
        )
        .unwrap();

        let first = db
            .generate_coding_improvement_proposals(&session.id, Some(30))
            .unwrap();
        let second = db
            .generate_coding_improvement_proposals(&session.id, Some(30))
            .unwrap();
        assert!(first.inserted > 0);
        assert_eq!(second.inserted, 0);
        assert!(second
            .proposals
            .iter()
            .any(|proposal| proposal.kind == "eval_candidate" && proposal.status == "draft"));
    }
}
