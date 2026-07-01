//! Session-scoped Goal control plane.
//!
//! A Goal is the durable "what are we trying to finish?" object above
//! workflow/task execution. It lives in `sessions.db` so it shares the same
//! lifecycle as sessions, workflow runs, and tasks.

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::session::{SessionDB, Task};
use crate::workflow::{WorkflowOpState, WorkflowRun, WorkflowRunState};

const GOAL_EVENT_PAYLOAD_MAX_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalState {
    Active,
    Paused,
    Evaluating,
    Completed,
    Failed,
    Cancelled,
    Blocked,
}

impl GoalState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Evaluating => "evaluating",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Blocked => "blocked",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "paused" => Some(Self::Paused),
            "evaluating" => Some(Self::Evaluating),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            "blocked" => Some(Self::Blocked),
            _ => None,
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    pub fn is_open(self) -> bool {
        matches!(
            self,
            Self::Active | Self::Paused | Self::Evaluating | Self::Blocked
        )
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        if self == next {
            return true;
        }
        match (self, next) {
            (
                Self::Active,
                Self::Paused
                | Self::Evaluating
                | Self::Completed
                | Self::Failed
                | Self::Cancelled
                | Self::Blocked,
            ) => true,
            (Self::Paused, Self::Active | Self::Evaluating | Self::Cancelled) => true,
            (
                Self::Evaluating,
                Self::Active | Self::Completed | Self::Failed | Self::Cancelled | Self::Blocked,
            ) => true,
            (Self::Blocked, Self::Active | Self::Evaluating | Self::Failed | Self::Cancelled) => {
                true
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Goal {
    pub id: String,
    pub session_id: String,
    pub objective: String,
    pub completion_criteria: String,
    pub state: GoalState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode_snapshot: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_token_limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_time_limit_secs: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_turn_limit: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_summary: Option<String>,
    pub final_evidence: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
    pub last_evaluator_result: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalEvent {
    pub id: i64,
    pub goal_id: String,
    pub seq: i64,
    pub kind: String,
    pub payload: Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalLink {
    pub id: i64,
    pub goal_id: String,
    pub target_type: String,
    pub target_id: String,
    pub relation: String,
    pub metadata: Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalSnapshot {
    pub goal: Goal,
    pub links: Vec<GoalLink>,
    pub events: Vec<GoalEvent>,
    #[serde(default)]
    pub workflow_runs: Vec<WorkflowRun>,
    #[serde(default)]
    pub tasks: Vec<Task>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateGoalInput {
    pub session_id: String,
    pub objective: String,
    #[serde(default)]
    pub completion_criteria: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_token_limit: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_time_limit_secs: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_turn_limit: Option<i64>,
}

pub(crate) fn ensure_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS goals (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            objective TEXT NOT NULL,
            completion_criteria TEXT NOT NULL DEFAULT '',
            state TEXT NOT NULL,
            mode_snapshot TEXT,
            budget_token_limit INTEGER,
            budget_time_limit_secs INTEGER,
            budget_turn_limit INTEGER,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            completed_at TEXT,
            final_summary TEXT,
            final_evidence_json TEXT NOT NULL DEFAULT '{}',
            blocked_reason TEXT,
            last_evaluator_result_json TEXT NOT NULL DEFAULT '{}',
            FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS goal_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            goal_id TEXT NOT NULL,
            seq INTEGER NOT NULL,
            kind TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY (goal_id) REFERENCES goals(id) ON DELETE CASCADE,
            UNIQUE(goal_id, seq)
        );

        CREATE TABLE IF NOT EXISTS goal_links (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            goal_id TEXT NOT NULL,
            target_type TEXT NOT NULL,
            target_id TEXT NOT NULL,
            relation TEXT NOT NULL,
            metadata_json TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL,
            FOREIGN KEY (goal_id) REFERENCES goals(id) ON DELETE CASCADE,
            UNIQUE(goal_id, target_type, target_id, relation)
        );

        CREATE INDEX IF NOT EXISTS idx_goals_session_updated
            ON goals(session_id, updated_at DESC);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_goals_session_open
            ON goals(session_id)
            WHERE state IN ('active','paused','evaluating','blocked');
        CREATE INDEX IF NOT EXISTS idx_goal_events_goal_seq
            ON goal_events(goal_id, seq);
        CREATE INDEX IF NOT EXISTS idx_goal_links_goal
            ON goal_links(goal_id);
        CREATE INDEX IF NOT EXISTS idx_goal_links_target
            ON goal_links(target_type, target_id);",
    )?;
    Ok(())
}

impl SessionDB {
    pub fn create_goal(&self, input: CreateGoalInput) -> Result<GoalSnapshot> {
        let objective = input.objective.trim();
        if objective.is_empty() {
            return Err(anyhow!("goal objective must not be empty"));
        }
        let criteria = input.completion_criteria.trim();
        let now = now_rfc3339();
        let id = format!("goal_{}", uuid::Uuid::new_v4().simple());
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        let (incognito, mode): (i64, String) = conn
            .query_row(
                "SELECT incognito, execution_mode FROM sessions WHERE id = ?1",
                params![input.session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| anyhow!("Session not found: {}", input.session_id))?;
        if incognito != 0 {
            return Err(anyhow!(
                "Cannot create durable goal for incognito session {}",
                input.session_id
            ));
        }
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM goals
                 WHERE session_id = ?1 AND state IN ('active','paused','evaluating','blocked')
                 LIMIT 1",
                params![input.session_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            return Err(anyhow!(
                "Session already has an open goal {}; clear or complete it first",
                existing
            ));
        }
        conn.execute(
            "INSERT INTO goals (
                id, session_id, objective, completion_criteria, state, mode_snapshot,
                budget_token_limit, budget_time_limit_secs, budget_turn_limit,
                created_at, updated_at, final_evidence_json, last_evaluator_result_json
            ) VALUES (?1, ?2, ?3, ?4, 'active', ?5, ?6, ?7, ?8, ?9, ?9, '{}', '{}')",
            params![
                id,
                input.session_id,
                objective,
                criteria,
                mode,
                input.budget_token_limit,
                input.budget_time_limit_secs,
                input.budget_turn_limit,
                now
            ],
        )?;
        drop(conn);
        let _ = self.append_goal_event(
            &id,
            "goal_created",
            json!({
                "objective": objective,
                "completionCriteria": criteria,
                "modeSnapshot": mode,
            }),
        )?;
        let snapshot = self
            .goal_snapshot(&id, 100)?
            .ok_or_else(|| anyhow!("goal {} was not persisted", id))?;
        emit_goal("goal:created", &snapshot.goal);
        Ok(snapshot)
    }

    pub fn get_goal(&self, goal_id: &str) -> Result<Option<Goal>> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        conn.query_row(
            "SELECT id, session_id, objective, completion_criteria, state, mode_snapshot,
                    budget_token_limit, budget_time_limit_secs, budget_turn_limit,
                    created_at, updated_at, completed_at, final_summary, final_evidence_json,
                    blocked_reason, last_evaluator_result_json
             FROM goals WHERE id = ?1",
            params![goal_id],
            row_to_goal,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn active_goal_for_session(&self, session_id: &str) -> Result<Option<GoalSnapshot>> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        let goal_id: Option<String> = conn
            .query_row(
                "SELECT id FROM goals
                 WHERE session_id = ?1 AND state IN ('active','paused','evaluating','blocked')
                 ORDER BY updated_at DESC
                 LIMIT 1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()?;
        drop(conn);
        match goal_id {
            Some(id) => self.goal_snapshot(&id, 100),
            None => Ok(None),
        }
    }

    pub fn active_goal_id_for_session(&self, session_id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        conn.query_row(
            "SELECT id FROM goals
             WHERE session_id = ?1 AND state IN ('active','paused','evaluating','blocked')
             ORDER BY updated_at DESC
             LIMIT 1",
            params![session_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn goal_snapshot(&self, goal_id: &str, event_limit: usize) -> Result<Option<GoalSnapshot>> {
        let Some(goal) = self.get_goal(goal_id)? else {
            return Ok(None);
        };
        let links = self.list_goal_links(goal_id)?;
        let events = self.list_goal_events(goal_id, event_limit)?;
        let workflow_runs = self.list_workflow_runs_for_goal(goal_id)?;
        let tasks = self.list_tasks(&goal.session_id).unwrap_or_default();
        Ok(Some(GoalSnapshot {
            goal,
            links,
            events,
            workflow_runs,
            tasks,
        }))
    }

    pub fn pause_goal(&self, goal_id: &str) -> Result<GoalSnapshot> {
        self.transition_goal(goal_id, GoalState::Paused, Some("pause_requested"))
    }

    pub fn resume_goal(&self, goal_id: &str) -> Result<GoalSnapshot> {
        self.transition_goal(goal_id, GoalState::Active, Some("resume_requested"))
    }

    pub fn clear_goal(&self, goal_id: &str) -> Result<GoalSnapshot> {
        self.transition_goal(goal_id, GoalState::Cancelled, Some("clear_requested"))
    }

    pub fn evaluate_goal(&self, goal_id: &str) -> Result<GoalSnapshot> {
        let _ = self.transition_goal(goal_id, GoalState::Evaluating, Some("evaluate_requested"))?;
        let snapshot = self
            .goal_snapshot(goal_id, 200)?
            .ok_or_else(|| anyhow!("goal {} not found", goal_id))?;
        let audit = self.build_goal_audit(&snapshot)?;
        let completed = audit
            .get("status")
            .and_then(|v| v.as_str())
            .is_some_and(|status| status == "completed");
        let next = if completed {
            GoalState::Completed
        } else {
            GoalState::Blocked
        };
        let summary = audit
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or(if completed {
                "Goal completed"
            } else {
                "Goal is not complete"
            })
            .to_string();
        let blocked_reason = if completed {
            None
        } else {
            Some(
                audit
                    .get("blockedReason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("goal_evidence_incomplete")
                    .to_string(),
            )
        };
        let now = now_rfc3339();
        let evidence_json = stable_json(&audit)?;
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        conn.execute(
            "UPDATE goals
                SET state = ?1,
                    updated_at = ?2,
                    completed_at = CASE WHEN ?1 IN ('completed','failed','cancelled') THEN ?2 ELSE completed_at END,
                    final_summary = ?3,
                    final_evidence_json = ?4,
                    blocked_reason = ?5,
                    last_evaluator_result_json = ?4
             WHERE id = ?6",
            params![
                next.as_str(),
                now,
                summary,
                evidence_json,
                blocked_reason,
                goal_id
            ],
        )?;
        drop(conn);
        let _ = self.append_goal_event(goal_id, "goal_evaluated", audit)?;
        let next_snapshot = self
            .goal_snapshot(goal_id, 200)?
            .ok_or_else(|| anyhow!("goal {} not found after evaluation", goal_id))?;
        emit_goal("goal:updated", &next_snapshot.goal);
        Ok(next_snapshot)
    }

    pub fn transition_goal(
        &self,
        goal_id: &str,
        next: GoalState,
        reason: Option<&str>,
    ) -> Result<GoalSnapshot> {
        let now = now_rfc3339();
        let previous = {
            let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
            let current: Option<String> = conn
                .query_row(
                    "SELECT state FROM goals WHERE id = ?1",
                    params![goal_id],
                    |row| row.get(0),
                )
                .optional()?;
            let current = current.ok_or_else(|| anyhow!("goal {} not found", goal_id))?;
            let previous = parse_goal_state(&current)?;
            if !previous.can_transition_to(next) {
                return Err(anyhow!(
                    "invalid goal transition {} -> {}",
                    previous.as_str(),
                    next.as_str()
                ));
            }
            conn.execute(
                "UPDATE goals
                    SET state = ?1,
                        blocked_reason = CASE WHEN ?1 = 'blocked' THEN ?2 ELSE NULL END,
                        completed_at = CASE WHEN ?1 IN ('completed','failed','cancelled') THEN ?3 ELSE completed_at END,
                        updated_at = ?3
                 WHERE id = ?4",
                params![next.as_str(), reason, now, goal_id],
            )?;
            previous
        };
        let _ = self.append_goal_event(
            goal_id,
            "goal_state_changed",
            json!({
                "from": previous.as_str(),
                "to": next.as_str(),
                "reason": reason,
            }),
        )?;
        let snapshot = self
            .goal_snapshot(goal_id, 100)?
            .ok_or_else(|| anyhow!("goal {} not found after transition", goal_id))?;
        emit_goal("goal:updated", &snapshot.goal);
        Ok(snapshot)
    }

    pub fn link_goal_target(
        &self,
        goal_id: &str,
        target_type: &str,
        target_id: &str,
        relation: &str,
        metadata: Value,
    ) -> Result<GoalLink> {
        let goal = self
            .get_goal(goal_id)?
            .ok_or_else(|| anyhow!("goal {} not found", goal_id))?;
        if goal.state.is_terminal() {
            return Err(anyhow!("goal {} is terminal", goal_id));
        }
        let now = now_rfc3339();
        let metadata_json = stable_json(&metadata)?;
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        conn.execute(
            "INSERT INTO goal_links (goal_id, target_type, target_id, relation, metadata_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(goal_id, target_type, target_id, relation)
             DO UPDATE SET metadata_json = excluded.metadata_json",
            params![goal_id, target_type, target_id, relation, metadata_json, now],
        )?;
        let id: i64 = conn.query_row(
            "SELECT id FROM goal_links
             WHERE goal_id = ?1 AND target_type = ?2 AND target_id = ?3 AND relation = ?4",
            params![goal_id, target_type, target_id, relation],
            |row| row.get(0),
        )?;
        drop(conn);
        let link = self
            .get_goal_link(id)?
            .ok_or_else(|| anyhow!("goal link {} not found after upsert", id))?;
        let _ = self.append_goal_event(
            goal_id,
            "goal_linked",
            json!({
                "targetType": target_type,
                "targetId": target_id,
                "relation": relation,
                "metadata": link.metadata,
            }),
        )?;
        emit_goal_link("goal:link_updated", &link);
        Ok(link)
    }

    pub fn append_goal_event(
        &self,
        goal_id: &str,
        kind: &str,
        payload: Value,
    ) -> Result<GoalEvent> {
        let payload_json = bounded_payload(payload)?;
        let now = now_rfc3339();
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        let seq: i64 = conn.query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM goal_events WHERE goal_id = ?1",
            params![goal_id],
            |row| row.get(0),
        )?;
        conn.execute(
            "INSERT INTO goal_events (goal_id, seq, kind, payload_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![goal_id, seq, kind, payload_json, now],
        )?;
        let id = conn.last_insert_rowid();
        let event = GoalEvent {
            id,
            goal_id: goal_id.to_string(),
            seq,
            kind: kind.to_string(),
            payload: serde_json::from_str(&payload_json)?,
            created_at: now,
        };
        drop(conn);
        emit_goal_event("goal:event", &event);
        Ok(event)
    }

    pub fn list_goal_events(&self, goal_id: &str, limit: usize) -> Result<Vec<GoalEvent>> {
        let limit = limit.clamp(1, 500) as i64;
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        let mut stmt = conn.prepare(
            "SELECT id, goal_id, seq, kind, payload_json, created_at
             FROM goal_events
             WHERE goal_id = ?1
             ORDER BY seq DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![goal_id, limit], row_to_goal_event)?;
        let mut events = collect_rows(rows)?;
        events.reverse();
        Ok(events)
    }

    pub fn list_goal_links(&self, goal_id: &str) -> Result<Vec<GoalLink>> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        let mut stmt = conn.prepare(
            "SELECT id, goal_id, target_type, target_id, relation, metadata_json, created_at
             FROM goal_links
             WHERE goal_id = ?1
             ORDER BY created_at ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![goal_id], row_to_goal_link)?;
        collect_rows(rows)
    }

    fn get_goal_link(&self, id: i64) -> Result<Option<GoalLink>> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        conn.query_row(
            "SELECT id, goal_id, target_type, target_id, relation, metadata_json, created_at
             FROM goal_links WHERE id = ?1",
            params![id],
            row_to_goal_link,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn list_workflow_runs_for_goal(&self, goal_id: &str) -> Result<Vec<WorkflowRun>> {
        let ids = {
            let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
            let mut stmt = conn.prepare(
                "SELECT id FROM workflow_runs
                 WHERE goal_id = ?1
                    OR id IN (
                        SELECT target_id FROM goal_links
                        WHERE goal_id = ?1 AND target_type = 'workflow_run'
                    )
                 ORDER BY updated_at DESC, created_at DESC",
            )?;
            let ids = stmt.query_map(params![goal_id], |row| row.get::<_, String>(0))?;
            collect_rows(ids)?
        };
        let mut runs = Vec::new();
        for id in ids {
            if let Some(run) = self.get_workflow_run(&id)? {
                runs.push(run);
            }
        }
        Ok(runs)
    }

    fn build_goal_audit(&self, snapshot: &GoalSnapshot) -> Result<Value> {
        let criteria = split_criteria(&snapshot.goal.completion_criteria);
        let mut achieved = Vec::new();
        let mut missing = Vec::new();
        let mut blockers = Vec::new();
        let mut evidence = Vec::new();

        if snapshot.workflow_runs.is_empty() && snapshot.tasks.is_empty() {
            blockers.push("No linked workflow run or task evidence yet.".to_string());
        }

        for task in &snapshot.tasks {
            if task.status == "completed" {
                achieved.push(format!("Task completed: {}", task.content));
            } else {
                missing.push(format!("Task not completed: {}", task.content));
            }
        }

        for run in &snapshot.workflow_runs {
            let run_label = format!("workflow {} ({})", run.id, run.state.as_str());
            match run.state {
                WorkflowRunState::Completed => {
                    achieved.push(format!("{run_label} completed"));
                    evidence.push(json!({
                        "type": "workflow_run",
                        "id": run.id,
                        "state": run.state,
                        "updatedAt": run.updated_at,
                    }));
                }
                WorkflowRunState::Failed
                | WorkflowRunState::Blocked
                | WorkflowRunState::Cancelled => {
                    blockers.push(format!(
                        "{run_label}: {}",
                        run.blocked_reason
                            .as_deref()
                            .unwrap_or("terminal without completion")
                    ));
                }
                WorkflowRunState::Draft
                | WorkflowRunState::AwaitingApproval
                | WorkflowRunState::Running
                | WorkflowRunState::AwaitingUser
                | WorkflowRunState::Paused
                | WorkflowRunState::Recovering => {
                    missing.push(format!("{run_label} is still in progress"));
                }
            }

            if let Some(run_snapshot) = self.workflow_run_snapshot(&run.id, 120)? {
                for op in run_snapshot.ops {
                    if op.op_type != "validate" {
                        continue;
                    }
                    let output = op.output.clone().unwrap_or(Value::Null);
                    let ok = output.get("ok").and_then(|v| v.as_bool());
                    match (op.state, ok) {
                        (WorkflowOpState::Completed, Some(true)) => {
                            achieved.push(format!("Validation passed in {}", run.id));
                            evidence.push(json!({
                                "type": "validation",
                                "runId": run.id,
                                "opKey": op.op_key,
                                "ok": true,
                                "summary": output.get("summary").cloned().unwrap_or(Value::Null),
                            }));
                        }
                        (WorkflowOpState::Completed, Some(false)) => {
                            blockers.push(format!(
                                "Validation failed in {}: {}",
                                run.id,
                                output
                                    .get("summary")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("no summary")
                            ));
                        }
                        (WorkflowOpState::Failed, _) => {
                            blockers.push(format!("Validation op failed in {}", run.id));
                        }
                        _ => {}
                    }
                }
            }
        }

        for criterion in &criteria {
            if !evidence.is_empty() || achieved.iter().any(|item| item.contains(criterion)) {
                achieved.push(format!("Criterion has supporting evidence: {criterion}"));
            } else {
                missing.push(format!("Criterion lacks direct evidence: {criterion}"));
            }
        }

        achieved.sort();
        achieved.dedup();
        missing.sort();
        missing.dedup();
        blockers.sort();
        blockers.dedup();

        let status = if blockers.is_empty() && missing.is_empty() && !evidence.is_empty() {
            "completed"
        } else {
            "blocked"
        };
        let summary = if status == "completed" {
            format!(
                "Goal completed with {} evidence item(s) and {} achieved item(s).",
                evidence.len(),
                achieved.len()
            )
        } else {
            format!(
                "Goal is not complete: {} blocker(s), {} missing item(s), {} evidence item(s).",
                blockers.len(),
                missing.len(),
                evidence.len()
            )
        };

        Ok(json!({
            "status": status,
            "summary": summary,
            "blockedReason": if blockers.is_empty() { "goal_evidence_incomplete" } else { "goal_blocked_by_evidence" },
            "objective": snapshot.goal.objective,
            "criteria": criteria,
            "achieved": achieved,
            "missing": missing,
            "blockers": blockers,
            "evidence": evidence,
            "remainingRisk": if status == "completed" {
                "Rule-based audit only; user can request a deeper review if the goal is high risk."
            } else {
                "More workflow/task/validation evidence is required before completion can be claimed."
            },
        }))
    }
}

fn split_criteria(raw: &str) -> Vec<String> {
    raw.lines()
        .flat_map(|line| line.split(';'))
        .map(|line| {
            line.trim()
                .trim_start_matches('-')
                .trim_start_matches('*')
                .trim()
                .to_string()
        })
        .filter(|line| !line.is_empty())
        .collect()
}

fn row_to_goal(row: &rusqlite::Row<'_>) -> rusqlite::Result<Goal> {
    let state: String = row.get(4)?;
    let final_evidence_json: String = row.get(13)?;
    let evaluator_json: String = row.get(15)?;
    Ok(Goal {
        id: row.get(0)?,
        session_id: row.get(1)?,
        objective: row.get(2)?,
        completion_criteria: row.get(3)?,
        state: parse_goal_state_sql(&state)?,
        mode_snapshot: row.get(5)?,
        budget_token_limit: row.get(6)?,
        budget_time_limit_secs: row.get(7)?,
        budget_turn_limit: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
        completed_at: row.get(11)?,
        final_summary: row.get(12)?,
        final_evidence: json_from_sql(&final_evidence_json)?,
        blocked_reason: row.get(14)?,
        last_evaluator_result: json_from_sql(&evaluator_json)?,
    })
}

fn row_to_goal_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<GoalEvent> {
    let payload_json: String = row.get(4)?;
    Ok(GoalEvent {
        id: row.get(0)?,
        goal_id: row.get(1)?,
        seq: row.get(2)?,
        kind: row.get(3)?,
        payload: json_from_sql(&payload_json)?,
        created_at: row.get(5)?,
    })
}

fn row_to_goal_link(row: &rusqlite::Row<'_>) -> rusqlite::Result<GoalLink> {
    let metadata_json: String = row.get(5)?;
    Ok(GoalLink {
        id: row.get(0)?,
        goal_id: row.get(1)?,
        target_type: row.get(2)?,
        target_id: row.get(3)?,
        relation: row.get(4)?,
        metadata: json_from_sql(&metadata_json)?,
        created_at: row.get(6)?,
    })
}

fn collect_rows<T>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>>,
) -> Result<Vec<T>> {
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn parse_goal_state(value: &str) -> Result<GoalState> {
    GoalState::from_str(value).ok_or_else(|| anyhow!("unknown goal state: {value}"))
}

fn parse_goal_state_sql(value: &str) -> rusqlite::Result<GoalState> {
    GoalState::from_str(value).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("unknown goal state: {value}").into(),
        )
    })
}

fn json_from_sql(value: &str) -> rusqlite::Result<Value> {
    serde_json::from_str(value).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, err.into())
    })
}

fn stable_json(value: &Value) -> Result<String> {
    serde_json::to_string(value).map_err(Into::into)
}

fn bounded_payload(payload: Value) -> Result<String> {
    let encoded = stable_json(&payload)?;
    if encoded.len() <= GOAL_EVENT_PAYLOAD_MAX_BYTES {
        return Ok(encoded);
    }
    let preview = crate::truncate_utf8(&encoded, GOAL_EVENT_PAYLOAD_MAX_BYTES);
    stable_json(&json!({
        "truncated": true,
        "preview": preview,
        "originalBytes": encoded.len(),
    }))
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::*;
    use crate::workflow::{CreateWorkflowRunInput, WorkflowRunState};

    fn temp_db() -> (tempfile::TempDir, SessionDB) {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = SessionDB::open(&dir.path().join("sessions.db")).expect("open session db");
        (dir, db)
    }

    fn create_goal_for_session(db: &SessionDB, session_id: &str) -> GoalSnapshot {
        db.create_goal(CreateGoalInput {
            session_id: session_id.to_string(),
            objective: "Ship goal mode".to_string(),
            completion_criteria: "workflow completes with evidence".to_string(),
            budget_token_limit: None,
            budget_time_limit_secs: None,
            budget_turn_limit: None,
        })
        .expect("create goal")
    }

    fn create_workflow(db: &SessionDB, session_id: &str, goal_id: Option<String>) -> WorkflowRun {
        db.create_workflow_run(CreateWorkflowRunInput {
            session_id: session_id.to_string(),
            kind: "coding.workflow".to_string(),
            execution_mode: "guarded".to_string(),
            script_source: "export default async function main(workflow) {}".to_string(),
            budget: json!({ "max_script_secs": 30, "max_ops": 8 }),
            parent_run_id: None,
            origin: None,
            goal_id,
        })
        .expect("create workflow")
    }

    #[test]
    fn create_goal_rejects_incognito_session() {
        let (_dir, db) = temp_db();
        let session = db
            .create_session_with_project("ha-main", None, Some(true))
            .expect("create incognito session");

        let err = db
            .create_goal(CreateGoalInput {
                session_id: session.id,
                objective: "Do not persist".to_string(),
                completion_criteria: String::new(),
                budget_token_limit: None,
                budget_time_limit_secs: None,
                budget_turn_limit: None,
            })
            .expect_err("incognito goal must be rejected");
        assert!(err.to_string().contains("incognito"));
    }

    #[test]
    fn workflow_creation_auto_links_active_goal() {
        let (_dir, db) = temp_db();
        let session = db.create_session("ha-main").expect("create session");
        let goal = create_goal_for_session(&db, &session.id);

        let run = create_workflow(&db, &session.id, None);
        assert_eq!(run.goal_id.as_deref(), Some(goal.goal.id.as_str()));

        let snapshot = db
            .goal_snapshot(&goal.goal.id, 100)
            .expect("goal snapshot")
            .expect("goal exists");
        assert!(snapshot.links.iter().any(|link| {
            link.target_type == "workflow_run"
                && link.target_id == run.id
                && link.relation == "execution_run"
        }));
    }

    #[test]
    fn workflow_completion_auto_evaluates_goal() {
        let (_dir, db) = temp_db();
        let session = db.create_session("ha-main").expect("create session");
        let goal = create_goal_for_session(&db, &session.id);
        let run = create_workflow(&db, &session.id, Some(goal.goal.id.clone()));

        db.transition_workflow_run(&run.id, WorkflowRunState::Running, Some("test_start"))
            .expect("start run");
        db.transition_workflow_run(&run.id, WorkflowRunState::Completed, Some("test_done"))
            .expect("complete run");

        let snapshot = db
            .goal_snapshot(&goal.goal.id, 200)
            .expect("goal snapshot")
            .expect("goal exists");
        assert_eq!(snapshot.goal.state, GoalState::Completed);
        assert_eq!(
            snapshot
                .goal
                .final_evidence
                .get("status")
                .and_then(Value::as_str),
            Some("completed")
        );
        assert!(snapshot.links.iter().any(|link| {
            link.target_type == "workflow_run"
                && link.target_id == run.id
                && link.relation == "workflow_completed"
        }));
    }

    #[test]
    fn goal_evaluate_blocks_without_evidence() {
        let (_dir, db) = temp_db();
        let session = db.create_session("ha-main").expect("create session");
        let goal = create_goal_for_session(&db, &session.id);

        let snapshot = db.evaluate_goal(&goal.goal.id).expect("evaluate goal");

        assert_eq!(snapshot.goal.state, GoalState::Blocked);
        assert_eq!(
            snapshot
                .goal
                .final_evidence
                .get("status")
                .and_then(Value::as_str),
            Some("blocked")
        );
    }
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn emit_goal<T: Serialize>(name: &str, payload: &T) {
    if let Some(bus) = crate::get_event_bus() {
        bus.emit(name, json!(payload));
    }
}

fn emit_goal_event(name: &str, event: &GoalEvent) {
    emit_goal(name, event);
}

fn emit_goal_link(name: &str, link: &GoalLink) {
    emit_goal(name, link);
}
