//! Feishu contact (联系人 / Lark Contact) — 4 LLM tools.
//!
//! **Sensitive data warning**: returns employee personal info (name /
//! email / mobile / department / job title / avatar). Surface this in
//! the tool descriptions so the agent treats results carefully.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::tools::definitions::{ToolDefinition, ToolTier};

use super::resolve_feishu_api;

pub const TOOL_CONTACT_GET_USER: &str = "feishu_contact_get_user";
pub const TOOL_CONTACT_BATCH_GET_USERS: &str = "feishu_contact_batch_get_users";
pub const TOOL_CONTACT_GET_DEPARTMENT: &str = "feishu_contact_get_department";
pub const TOOL_CONTACT_SEARCH_USERS_BY_DEPARTMENT: &str =
    "feishu_contact_search_users_by_department";

const HINT: &str =
    "Configure a Feishu IM channel account in Settings → Channels to enable contact tools.";

const SENSITIVE_HINT: &str = " ⚠️ Returns employee personal info (name / email / mobile / department / job_title / avatar). Treat results as sensitive — don't echo verbatim into untrusted contexts.";

fn account_param() -> Value {
    json!({"type": "string", "description": "Feishu channel account ID; required only with multiple accounts."})
}
fn cfg() -> ToolTier {
    ToolTier::Configured {
        default_for_main: false,
        default_for_others: false,
        default_deferred: true,
        config_hint: HINT,
    }
}

pub fn get_user_tool() -> ToolDefinition {
    ToolDefinition {
        name: TOOL_CONTACT_GET_USER.into(),
        description: format!(
            "Fetch a Feishu (Lark) user's profile by user_id. Required Feishu app scope: \
             `contact:user.id:readonly` or stronger.{}",
            SENSITIVE_HINT
        ),
        tier: cfg(),
        internal: false,
        concurrent_safe: true,
        async_capable: false,
        parameters: json!({
            "type": "object",
            "properties": {
                "user_id": {"type": "string"},
                "user_id_type": {"type": "string", "description": "`open_id` (default) / `union_id` / `user_id`."},
                "account": account_param(),
            },
            "required": ["user_id"],
            "additionalProperties": false
        }),
    }
}

pub fn batch_get_users_tool() -> ToolDefinition {
    ToolDefinition {
        name: TOOL_CONTACT_BATCH_GET_USERS.into(),
        description: format!(
            "Fetch up to 50 Feishu (Lark) users in one call. Required Feishu app scope: \
             `contact:user.id:readonly` or stronger.{}",
            SENSITIVE_HINT
        ),
        tier: cfg(),
        internal: false,
        concurrent_safe: true,
        async_capable: false,
        parameters: json!({
            "type": "object",
            "properties": {
                "user_ids": {"type": "array", "items": {"type": "string"}, "description": "1-50 user IDs."},
                "user_id_type": {"type": "string"},
                "account": account_param(),
            },
            "required": ["user_ids"],
            "additionalProperties": false
        }),
    }
}

pub fn get_department_tool() -> ToolDefinition {
    ToolDefinition {
        name: TOOL_CONTACT_GET_DEPARTMENT.into(),
        description:
            "Fetch a Feishu (Lark) department's info by department_id (name, parent, member \
             count, leader). Required Feishu app scope: `contact:department.id:readonly` or \
             stronger."
                .into(),
        tier: cfg(),
        internal: false,
        concurrent_safe: true,
        async_capable: false,
        parameters: json!({
            "type": "object",
            "properties": {
                "department_id": {"type": "string"},
                "department_id_type": {"type": "string", "description": "`open_department_id` (default) / `department_id`."},
                "account": account_param(),
            },
            "required": ["department_id"],
            "additionalProperties": false
        }),
    }
}

pub fn search_users_by_department_tool() -> ToolDefinition {
    ToolDefinition {
        name: TOOL_CONTACT_SEARCH_USERS_BY_DEPARTMENT.into(),
        description: format!(
            "List Feishu (Lark) users belonging to a department, paginated. Useful for \
             enumerating a team. Required Feishu app scope: `contact:user.id:readonly` or \
             stronger.{}",
            SENSITIVE_HINT
        ),
        tier: cfg(),
        internal: false,
        concurrent_safe: true,
        async_capable: false,
        parameters: json!({
            "type": "object",
            "properties": {
                "department_id": {"type": "string"},
                "page_token": {"type": "string"},
                "page_size": {"type": "integer"},
                "account": account_param(),
            },
            "required": ["department_id"],
            "additionalProperties": false
        }),
    }
}

fn s<'a>(args: &'a Value, k: &str) -> Option<&'a str> {
    args.get(k).and_then(|v| v.as_str())
}
fn rs<'a>(args: &'a Value, k: &str) -> Result<&'a str> {
    s(args, k).ok_or_else(|| anyhow!("`{}` is required and must be a string", k))
}
fn ras(args: &Value, k: &str) -> Result<Vec<String>> {
    let arr = args
        .get(k)
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("`{}` is required and must be an array of strings", k))?;
    let mut out = Vec::with_capacity(arr.len());
    for (i, v) in arr.iter().enumerate() {
        let s = v
            .as_str()
            .ok_or_else(|| anyhow!("`{}[{}]` must be a string", k, i))?;
        out.push(s.to_string());
    }
    Ok(out)
}
fn u32_opt(args: &Value, k: &str) -> Result<Option<u32>> {
    match args.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => n
            .as_u64()
            .and_then(|x| u32::try_from(x).ok())
            .map(Some)
            .ok_or_else(|| anyhow!("`{}` must fit u32", k)),
        _ => Err(anyhow!("`{}` must be an integer", k)),
    }
}

pub(crate) async fn execute_get_user(args: &Value) -> Result<String> {
    let api = resolve_feishu_api(s(args, "account")).await?;
    let u = api
        .contact_get_user(rs(args, "user_id")?, s(args, "user_id_type"))
        .await?;
    Ok(serde_json::to_string(&u)?)
}
pub(crate) async fn execute_batch_get_users(args: &Value) -> Result<String> {
    let api = resolve_feishu_api(s(args, "account")).await?;
    let r = api
        .contact_batch_get_users(ras(args, "user_ids")?, s(args, "user_id_type"))
        .await?;
    Ok(serde_json::to_string(&r)?)
}
pub(crate) async fn execute_get_department(args: &Value) -> Result<String> {
    let api = resolve_feishu_api(s(args, "account")).await?;
    let d = api
        .contact_get_department(rs(args, "department_id")?, s(args, "department_id_type"))
        .await?;
    Ok(serde_json::to_string(&d)?)
}
pub(crate) async fn execute_search_users_by_department(args: &Value) -> Result<String> {
    let api = resolve_feishu_api(s(args, "account")).await?;
    let r = api
        .contact_search_users_by_department(
            rs(args, "department_id")?,
            s(args, "page_token"),
            u32_opt(args, "page_size")?,
        )
        .await?;
    Ok(serde_json::to_string(&r)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_match() {
        assert_eq!(get_user_tool().name, TOOL_CONTACT_GET_USER);
        assert_eq!(batch_get_users_tool().name, TOOL_CONTACT_BATCH_GET_USERS);
        assert_eq!(get_department_tool().name, TOOL_CONTACT_GET_DEPARTMENT);
        assert_eq!(
            search_users_by_department_tool().name,
            TOOL_CONTACT_SEARCH_USERS_BY_DEPARTMENT
        );
    }
    #[test]
    fn user_tools_flag_sensitive_data() {
        assert!(get_user_tool().description.contains("personal info"));
        assert!(batch_get_users_tool().description.contains("personal info"));
        assert!(search_users_by_department_tool()
            .description
            .contains("personal info"));
    }
    #[tokio::test]
    async fn batch_requires_user_ids_array() {
        let err = execute_batch_get_users(&json!({})).await.unwrap_err();
        assert!(err.to_string().contains("user_ids"), "{}", err);
    }
}
