use std::sync::Arc;

use crate::goal::{CreateGoalInput, GoalSnapshot, GoalState};
use crate::session::SessionDB;
use crate::slash_commands::types::{CommandAction, CommandResult};

pub fn handle_goal(
    session_db: &Arc<SessionDB>,
    session_id: Option<&str>,
    args: &str,
) -> Result<CommandResult, String> {
    let sid = session_id.ok_or("No active session")?;
    let trimmed = args.trim();
    if trimmed.is_empty() || matches!(trimmed, "status" | "show") {
        return render_active_goal(session_db, sid);
    }
    if trimmed == "help" {
        return Ok(display_only(goal_usage()));
    }

    let mut parts = trimmed.split_whitespace();
    let command = parts.next().unwrap_or_default();
    match command {
        "pause" => transition_active_goal(session_db, sid, GoalCommand::Pause),
        "resume" => transition_active_goal(session_db, sid, GoalCommand::Resume),
        "clear" | "cancel" => transition_active_goal(session_db, sid, GoalCommand::Clear),
        "evaluate" | "audit" => transition_active_goal(session_db, sid, GoalCommand::Evaluate),
        "status" | "show" => render_active_goal(session_db, sid),
        "set" | "create" => create_goal(session_db, sid, trimmed[command.len()..].trim()),
        _ => create_goal(session_db, sid, trimmed),
    }
}

enum GoalCommand {
    Pause,
    Resume,
    Clear,
    Evaluate,
}

fn create_goal(session_db: &Arc<SessionDB>, sid: &str, raw: &str) -> Result<CommandResult, String> {
    let (objective, completion_criteria) = parse_goal_create_args(raw);
    if objective.trim().is_empty() {
        return Err(goal_usage());
    }
    let snapshot = session_db
        .create_goal(CreateGoalInput {
            session_id: sid.to_string(),
            objective,
            completion_criteria,
            budget_token_limit: None,
            budget_time_limit_secs: None,
            budget_turn_limit: None,
        })
        .map_err(|e| e.to_string())?;
    Ok(display_only(render_goal_snapshot(&snapshot)))
}

fn transition_active_goal(
    session_db: &Arc<SessionDB>,
    sid: &str,
    command: GoalCommand,
) -> Result<CommandResult, String> {
    let snapshot = session_db
        .active_goal_for_session(sid)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            "No active goal for this session. Use `/goal <objective> --criteria <criteria>`."
                .to_string()
        })?;
    let next = match command {
        GoalCommand::Pause => session_db.pause_goal(&snapshot.goal.id),
        GoalCommand::Resume => session_db.resume_goal(&snapshot.goal.id),
        GoalCommand::Clear => session_db.clear_goal(&snapshot.goal.id),
        GoalCommand::Evaluate => session_db.evaluate_goal(&snapshot.goal.id),
    }
    .map_err(|e| e.to_string())?;
    Ok(display_only(render_goal_snapshot(&next)))
}

fn render_active_goal(session_db: &Arc<SessionDB>, sid: &str) -> Result<CommandResult, String> {
    match session_db.active_goal_for_session(sid).map_err(|e| e.to_string())? {
        Some(snapshot) => Ok(display_only(render_goal_snapshot(&snapshot))),
        None => Ok(display_only(
            "No active goal for this session.\n\nUse `/goal <objective> --criteria <completion criteria>` to create one.",
        )),
    }
}

fn parse_goal_create_args(raw: &str) -> (String, String) {
    let markers = [
        "--criteria",
        "criteria:",
        "completion criteria:",
        "完成标准：",
        "完成标准:",
    ];
    let lower = raw.to_lowercase();
    for marker in markers {
        let needle = marker.to_lowercase();
        if let Some(index) = lower.find(&needle) {
            let objective = raw[..index].trim().trim_matches('-').trim().to_string();
            let criteria = raw[index + marker.len()..]
                .trim()
                .trim_start_matches(':')
                .trim()
                .to_string();
            return (objective, criteria);
        }
    }
    (raw.trim().to_string(), String::new())
}

fn render_goal_snapshot(snapshot: &GoalSnapshot) -> String {
    let goal = &snapshot.goal;
    let state = goal_state_label(goal.state);
    let criteria = if goal.completion_criteria.trim().is_empty() {
        "_No explicit completion criteria yet._".to_string()
    } else {
        goal.completion_criteria
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(|line| format!("- {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let workflows = snapshot.workflow_runs.len();
    let tasks_total = snapshot.tasks.len();
    let tasks_done = snapshot
        .tasks
        .iter()
        .filter(|task| task.status == "completed")
        .count();
    let final_summary = goal
        .final_summary
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("No final audit yet.");
    let blocked = goal
        .blocked_reason
        .as_deref()
        .map(|reason| format!("\nBlocked reason: `{reason}`"))
        .unwrap_or_default();

    format!(
        "## Goal `{}`\n\nState: **{}** · workflows: **{}** · tasks: **{}/{}**{}\n\n**Objective**\n{}\n\n**Completion criteria**\n{}\n\n**Final audit**\n{}\n\nUse `/goal evaluate` to run the conservative final audit, `/goal pause|resume|clear` to control it.",
        short_id(&goal.id),
        state,
        workflows,
        tasks_done,
        tasks_total,
        blocked,
        goal.objective,
        criteria,
        final_summary,
    )
}

fn goal_state_label(state: GoalState) -> &'static str {
    match state {
        GoalState::Active => "active",
        GoalState::Paused => "paused",
        GoalState::Evaluating => "evaluating",
        GoalState::Completed => "completed",
        GoalState::Failed => "failed",
        GoalState::Cancelled => "cancelled",
        GoalState::Blocked => "blocked",
    }
}

fn goal_usage() -> String {
    [
        "## Goal commands",
        "",
        "- `/goal <objective> --criteria <completion criteria>`: create an active goal",
        "- `/goal`: show the active goal",
        "- `/goal status`: show the active goal",
        "- `/goal pause`: pause the active goal",
        "- `/goal resume`: resume the active/blocked goal",
        "- `/goal evaluate`: run final audit from linked workflow/task/validation evidence",
        "- `/goal clear`: cancel the active goal",
    ]
    .join("\n")
}

fn display_only(content: impl Into<String>) -> CommandResult {
    CommandResult {
        content: content.into(),
        action: Some(CommandAction::DisplayOnly),
    }
}

fn short_id(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}
