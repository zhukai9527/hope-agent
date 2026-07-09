use serde_json::json;

use super::super::{
    TOOL_LOOP_RECORD_PROGRESS, TOOL_LOOP_RESCHEDULE, TOOL_LOOP_STATUS, TOOL_LOOP_STOP,
};
use super::types::{CoreSubclass, ToolDefinition, ToolTier};

fn loop_core_tool(
    name: &str,
    description: &str,
    parameters: serde_json::Value,
    concurrent_safe: bool,
) -> ToolDefinition {
    ToolDefinition {
        name: name.into(),
        description: description.into(),
        tier: ToolTier::Core {
            subclass: CoreSubclass::Interaction,
        },
        internal: true,
        concurrent_safe,
        async_capable: false,
        parameters,
    }
}

pub fn get_loop_status_tool() -> ToolDefinition {
    loop_core_tool(
        TOOL_LOOP_STATUS,
        "Read durable Loop schedules for this session. Use this before deciding whether a \
dynamic loop should continue, reschedule, stop, or report blocked. This is read-only and returns \
a compact status plus recent run trace for one loop when a loopId is provided.",
        json!({
            "type": "object",
            "properties": {
                "loopId": {
                    "type": "string",
                    "description": "Optional exact loop id or short id prefix. Omit to list loops for the current session."
                }
            },
            "additionalProperties": false
        }),
        true,
    )
}

pub fn get_loop_reschedule_tool() -> ToolDefinition {
    loop_core_tool(
        TOOL_LOOP_RESCHEDULE,
        "Choose the next wakeup for an active dynamic Loop. Use this near the end of a loop \
iteration instead of relying on textual LOOP_RESCHEDULE_AFTER markers. The runtime clamps delaySecs \
to the dynamic-loop safety window and records the decision in the current loop run trace.",
        json!({
            "type": "object",
            "properties": {
                "loopId": {
                    "type": "string",
                    "description": "Optional exact loop id or short id prefix. Omit only when this session has one active dynamic loop."
                },
                "delaySecs": {
                    "type": "integer",
                    "description": "Requested delay before the next loop wakeup, in seconds. Values are clamped to 60..3600."
                },
                "reason": {
                    "type": "string",
                    "description": "Short reason for the chosen delay."
                }
            },
            "required": ["delaySecs", "reason"],
            "additionalProperties": false
        }),
        false,
    )
}

pub fn get_loop_stop_tool() -> ToolDefinition {
    loop_core_tool(
        TOOL_LOOP_STOP,
        "Stop the active Loop because it is complete or blocked. Use outcome=completed when the \
loop's recurring objective is done, and outcome=blocked when continuation needs user input or an \
external state change. This pauses the underlying Cron job through the Loop control plane.",
        json!({
            "type": "object",
            "properties": {
                "loopId": {
                    "type": "string",
                    "description": "Optional exact loop id or short id prefix. Omit only when this session has one active loop."
                },
                "outcome": {
                    "type": "string",
                    "enum": ["completed", "blocked"],
                    "description": "Whether the loop should finish as completed or blocked. Defaults to completed."
                },
                "reason": {
                    "type": "string",
                    "description": "Concise reason shown in trace/status."
                }
            },
            "required": ["reason"],
            "additionalProperties": false
        }),
        false,
    )
}

pub fn get_loop_record_progress_tool() -> ToolDefinition {
    loop_core_tool(
        TOOL_LOOP_RECORD_PROGRESS,
        "Record a lightweight progress note for a Loop. Use this for phase updates, observed \
state, or handoff notes. This does not count as strong completion evidence and does not bypass \
Goal/Loop progress guards; durable evidence should still come from workflows, tasks, validation, \
files, sources, or domain-quality checks.",
        json!({
            "type": "object",
            "properties": {
                "loopId": {
                    "type": "string",
                    "description": "Optional exact loop id or short id prefix. Omit only when this session has one active loop."
                },
                "state": {
                    "type": "string",
                    "enum": ["progressed", "weak_progress", "no_progress", "blocked", "failed", "awaiting_approval"],
                    "description": "Observed progress state."
                },
                "summary": {
                    "type": "string",
                    "description": "Short progress summary."
                },
                "reason": {
                    "type": "string",
                    "description": "Optional reason or handoff detail."
                },
                "metadata": {
                    "type": "object",
                    "description": "Small structured details. Do not include secrets or large raw artifacts.",
                    "additionalProperties": true
                }
            },
            "required": ["summary"],
            "additionalProperties": false
        }),
        false,
    )
}
