use anyhow::{bail, Result};
use serde_json::Value;

pub(crate) async fn tool_mac_control(args: &Value) -> Result<String> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("status");

    match action {
        "status" => Ok(serde_json::to_string_pretty(
            &crate::mac_control::status().await,
        )?),
        "permissions" => Ok(serde_json::to_string_pretty(
            &crate::mac_control::permissions().await,
        )?),
        other => bail!(
            "Unsupported mac_control action '{}'. Phase 1 supports only 'status' and 'permissions'.",
            other
        ),
    }
}
