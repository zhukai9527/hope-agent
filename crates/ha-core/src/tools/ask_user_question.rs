//! Execution backend for the `ask_user_question` tool.
//!
//! Types and the pending-question registry live in [`crate::ask_user`]; this
//! module only handles tool-call execution: parsing args, persisting the
//! group, awaiting the answer (with timeout / default fallback), and
//! formatting the result for the LLM.

use crate::ask_user::{
    self, AskUserI18nText, AskUserQuestion, AskUserQuestionAnswer, AskUserQuestionGroup,
    AskUserQuestionOption, AskUserText,
};
use crate::process_registry::create_session_id;
use serde_json::json;
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Execute the ask_user_question tool.
/// Sends structured questions to the user and blocks until they respond or time out.
pub(crate) async fn execute(args: &Value, session_id: Option<&str>) -> String {
    let sid = match session_id {
        Some(s) => s,
        None => return "Error: no session context available".to_string(),
    };

    let questions_val = match args.get("questions").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return "Error: questions parameter is required (array)".to_string(),
    };

    let context = parse_optional_text_field(args, "context");

    let mut questions = Vec::new();
    for (i, q) in questions_val.iter().enumerate() {
        let text = match parse_optional_text_field(q, "text") {
            Some(t) => t,
            None => {
                return format!(
                    "Error: questions[{}].text is required (string or {{key, params, fallback}})",
                    i
                )
            }
        };

        let options = q
            .get("options")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|opt| {
                        let value = opt.get("value").and_then(|v| v.as_str())?.to_string();
                        let label = parse_optional_text_field(opt, "label")?;
                        let description = opt.get("description").and_then(parse_text_value);
                        let recommended = opt
                            .get("recommended")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let preview = opt
                            .get("preview")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        let preview_kind = opt
                            .get("previewKind")
                            .or_else(|| opt.get("preview_kind"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        Some(AskUserQuestionOption {
                            value,
                            label,
                            description,
                            recommended,
                            preview,
                            preview_kind,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        // 模型可传 `allow_custom` 参数，但当前强制覆盖为 true：
        // 模型给的选项常常覆盖不到用户真实意图，强制留一个自由文本入口
        // 避免用户被迫二选一。字段和 schema 都保留着，等未来模型提问质量
        // 更稳定后可以摘掉这段覆盖恢复模型自主控制。
        let _model_allow_custom = q.get("allow_custom").and_then(|v| v.as_bool());
        let allow_custom = true;
        let multi_select = q
            .get("multi_select")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let question_id = q
            .get("question_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("q_{}", i));

        let template = q
            .get("template")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let header = q.get("header").and_then(parse_text_value);
        let timeout_secs = q
            .get("timeout_secs")
            .or_else(|| q.get("timeoutSecs"))
            .and_then(|v| v.as_u64())
            .filter(|n| *n > 0);
        let default_values = q
            .get("default_values")
            .or_else(|| q.get("defaultValues"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        questions.push(AskUserQuestion {
            question_id,
            text,
            options,
            allow_custom,
            multi_select,
            template,
            header,
            timeout_secs,
            default_values,
        });
    }

    if questions.is_empty() {
        return "Error: at least one question is required".to_string();
    }

    let request_id = create_session_id();

    // Route to parent session if this is a plan sub-agent. Cache the lookup
    // so the `source` tag can reuse it without a second DB round-trip.
    let plan_owner = crate::plan::get_plan_owner_session_id(sid).await;
    let effective_sid = plan_owner.clone().unwrap_or_else(|| sid.to_string());
    let source = Some(
        if plan_owner.is_some() {
            "plan"
        } else {
            "normal"
        }
        .to_string(),
    );

    // Resolve effective group timeout: max(per-question timeouts, global default).
    let global_default = crate::config::cached_config().ask_user_question_timeout_secs;
    let per_q_max = questions
        .iter()
        .filter_map(|q| q.timeout_secs)
        .max()
        .unwrap_or(0);
    let effective_timeout_secs = if per_q_max > 0 {
        per_q_max
    } else {
        global_default
    };
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let timeout_at = if effective_timeout_secs > 0 {
        Some(now_secs + effective_timeout_secs)
    } else {
        None
    };

    let group = AskUserQuestionGroup {
        request_id: request_id.clone(),
        session_id: effective_sid.clone(),
        questions: questions.clone(),
        context: context.clone(),
        source,
        timeout_at,
    };

    // Persist the pending group before emitting so restarts can resume it.
    if let Err(e) = ask_user::persist_pending_group(&group) {
        app_warn!(
            "ask_user",
            "persist",
            "Failed to persist pending ask_user group {}: {}",
            request_id,
            e
        );
    }

    // Create oneshot channel + register pending.
    let (tx, rx) = tokio::sync::oneshot::channel();
    ask_user::register_ask_user_question(request_id.clone(), tx).await;

    // Emit event.
    if let Some(bus) = crate::globals::get_event_bus() {
        match serde_json::to_value(&group) {
            Ok(event_data) => {
                bus.emit(ask_user::EVENT_ASK_USER_REQUEST, event_data);
                // Elicitation hook (decision-capable): let a hook (e.g. a
                // desktop pet) render an interactive answer card and answer the
                // prompt. Emitted in Claude's AskUserQuestion shape so the
                // consumer reuses its existing card with zero special-casing.
                // Spawned so the tool keeps waiting on `rx`; the hook's answers
                // are fed back through the SAME idempotent
                // `submit_ask_user_question_response`, so whichever source
                // (GUI / IM / hook) answers first wins and the rest are no-ops.
                {
                    let request_id = request_id.clone();
                    let sid = effective_sid.clone();
                    let questions = questions.clone();
                    let tool_input = build_claude_ask_user_tool_input(&questions, context.as_ref());
                    let max_wait = (effective_timeout_secs > 0)
                        .then(|| Duration::from_secs(effective_timeout_secs));
                    tokio::spawn(async move {
                        let outcome = crate::hooks::dispatch_elicitation(
                            &sid,
                            &request_id,
                            tool_input,
                            max_wait,
                        )
                        .await;
                        if let Some(answers) = outcome
                            .updated_input
                            .as_ref()
                            .and_then(|ui| ui.get("answers"))
                            .and_then(|a| map_claude_answers_to_ha(&questions, a))
                        {
                            let _ =
                                ask_user::submit_ask_user_question_response(&request_id, answers)
                                    .await;
                        }
                    });
                }
                app_info!(
                    "ask_user",
                    "emit",
                    "ask_user question emitted (id: {}, {} questions, timeout: {}s)",
                    request_id,
                    questions.len(),
                    effective_timeout_secs
                );
            }
            Err(e) => {
                ask_user::cancel_pending_ask_user_question(&request_id).await;
                let _ = ask_user::mark_group_answered(&request_id);
                return format!("Error: failed to serialize question: {}", e);
            }
        }
    } else {
        ask_user::cancel_pending_ask_user_question(&request_id).await;
        let _ = ask_user::mark_group_answered(&request_id);
        return "Error: EventBus not available for ask_user events".to_string();
    }

    // Wait for response with optional timeout.
    let result = if effective_timeout_secs == 0 {
        match rx.await {
            Ok(answers) => Outcome::Answered(answers),
            Err(_) => Outcome::Cancelled,
        }
    } else {
        match tokio::time::timeout(Duration::from_secs(effective_timeout_secs), rx).await {
            Ok(Ok(answers)) => Outcome::Answered(answers),
            Ok(Err(_)) => Outcome::Cancelled,
            Err(_) => {
                ask_user::cancel_pending_ask_user_question(&request_id).await;
                Outcome::TimedOut
            }
        }
    };

    // Final cleanup: mark persisted row answered and drop any IM-side pending
    // state so stale entries don't accumulate in the button/text maps.
    let _ = ask_user::mark_group_answered(&request_id);
    crate::channel::worker::ask_user::drop_pending_by_request_id(&request_id).await;

    // ElicitationResult hook (observation): the question group reached a
    // terminal state.
    let result_status = match &result {
        Outcome::Answered(_) => "answered",
        Outcome::Cancelled => "cancelled",
        Outcome::TimedOut => "timeout",
    };
    crate::hooks::fire_elicitation_result(&effective_sid, &request_id, result_status);

    // Tell the frontend this group is terminal so the GUI ask-user block
    // dismisses — for ANY path, not just the local GUI submit. The block is
    // otherwise only cleared by its own submit callback, so an answer from IM,
    // a desktop-pet hook, or a timeout would leave a stale, still-interactive
    // card on screen. Matched on the (globally unique) `requestId`.
    if let Some(bus) = crate::globals::get_event_bus() {
        bus.emit(
            "ask_user_resolved",
            serde_json::json!({
                "requestId": request_id,
                "sessionId": effective_sid,
                "status": result_status,
            }),
        );
    }

    match result {
        Outcome::Answered(answers) => {
            format_answers_for_llm(&questions, &answers, /* timed_out */ false)
        }
        Outcome::Cancelled => {
            app_warn!(
                "ask_user",
                "cancel",
                "ask_user question cancelled (id: {})",
                request_id
            );
            "The user cancelled the questions without answering.".to_string()
        }
        Outcome::TimedOut => {
            app_warn!(
                "ask_user",
                "timeout",
                "ask_user question timed out after {}s (id: {})",
                effective_timeout_secs,
                request_id
            );
            let synth = synthesize_default_answers(&questions);
            if synth.is_empty() {
                format!(
                    "The questions timed out after {} seconds without a response and no default values were provided.",
                    effective_timeout_secs
                )
            } else {
                format_answers_for_llm(&questions, &synth, /* timed_out */ true)
            }
        }
    }
}

enum Outcome {
    Answered(Vec<AskUserQuestionAnswer>),
    Cancelled,
    TimedOut,
}

/// Construct synthetic answers from each question's `default_values` after a timeout.
fn synthesize_default_answers(questions: &[AskUserQuestion]) -> Vec<AskUserQuestionAnswer> {
    let mut out = Vec::new();
    for q in questions {
        if q.default_values.is_empty() {
            continue;
        }
        let mut selected = Vec::new();
        let mut custom: Option<String> = None;
        for v in &q.default_values {
            if q.options.iter().any(|o| &o.value == v) {
                selected.push(v.clone());
            } else {
                custom = Some(match custom {
                    Some(prev) => format!("{prev}, {v}"),
                    None => v.clone(),
                });
            }
        }
        out.push(AskUserQuestionAnswer {
            question_id: q.question_id.clone(),
            selected,
            custom_input: custom,
        });
    }
    out
}

/// Format user answers as JSON for both LLM consumption and frontend rendering.
fn format_answers_for_llm(
    questions: &[AskUserQuestion],
    answers: &[AskUserQuestionAnswer],
    timed_out: bool,
) -> String {
    let mut items = Vec::new();
    for question in questions {
        let mut selected_labels = Vec::new();
        let mut custom_input: Option<String> = None;

        if let Some(answer) = answers
            .iter()
            .find(|a| a.question_id == question.question_id)
        {
            for sel in &answer.selected {
                let label = question
                    .options
                    .iter()
                    .find(|o| o.value == *sel)
                    .map(|o| o.label.fallback_text().to_string())
                    .unwrap_or_else(|| sel.clone());
                selected_labels.push(label);
            }
            if let Some(c) = &answer.custom_input {
                if !c.is_empty() {
                    custom_input = Some(c.clone());
                }
            }
        }

        items.push(serde_json::json!({
            "question": question.text.fallback_text(),
            "selected": selected_labels,
            "customInput": custom_input,
        }));
    }

    let mut root = serde_json::Map::new();
    root.insert("answers".into(), serde_json::Value::Array(items));
    if timed_out {
        root.insert("timedOut".into(), serde_json::Value::Bool(true));
        root.insert(
            "note".into(),
            serde_json::Value::String(
                "Some or all questions timed out; default values were automatically applied."
                    .into(),
            ),
        );
    }
    serde_json::Value::Object(root).to_string()
}

fn parse_optional_text_field(value: &Value, field: &str) -> Option<AskUserText> {
    value.get(field).and_then(parse_text_value)
}

fn parse_text_value(value: &Value) -> Option<AskUserText> {
    match value {
        Value::String(s) => Some(AskUserText::plain(s.clone())),
        Value::Object(obj) => {
            let key = obj.get("key")?.as_str()?.to_string();
            let fallback = obj
                .get("fallback")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let params = obj
                .get("params")
                .and_then(|v| v.as_object())
                .map(|m| {
                    m.iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect::<BTreeMap<_, _>>()
                })
                .unwrap_or_default();
            Some(AskUserText::I18n(AskUserI18nText {
                key,
                params,
                fallback,
            }))
        }
        _ => None,
    }
}

pub(super) fn i18n_text(key: &str, params: Value, fallback: impl Into<String>) -> Value {
    let params = params.as_object().cloned().unwrap_or_default();
    json!({
        "key": key,
        "params": params,
        "fallback": fallback.into(),
    })
}

// ── Claude-AskUserQuestion shape mapping (interactive elicitation hook) ──
//
// The `Elicitation` hook is emitted in Claude Code's AskUserQuestion shape so a
// Claude-aligned consumer (e.g. a desktop pet) renders its existing answer card
// with zero special-casing, and answers via the same `updatedInput.answers`
// channel. These two helpers translate between Hope Agent's richer question
// schema and that flatter Claude shape.

/// Map Hope Agent's questions to Claude's AskUserQuestion `tool_input`
/// (`{ questions: [{ question, options: [{label, description}], multiSelect }],
/// context? }`). The option `label` is the human text the consumer both shows
/// and echoes back as the answer; its canonical `value` is recovered from the
/// label in [`map_claude_answers_to_ha`].
fn build_claude_ask_user_tool_input(
    questions: &[AskUserQuestion],
    context: Option<&AskUserText>,
) -> Value {
    let mapped: Vec<Value> = questions
        .iter()
        .map(|q| {
            let options: Vec<Value> = q
                .options
                .iter()
                .map(|o| {
                    let mut obj = serde_json::Map::new();
                    obj.insert("label".into(), json!(o.label.fallback_text()));
                    if let Some(desc) = o.description.as_ref() {
                        obj.insert("description".into(), json!(desc.fallback_text()));
                    }
                    Value::Object(obj)
                })
                .collect();
            json!({
                "question": q.text.fallback_text(),
                "options": options,
                "multiSelect": q.multi_select,
            })
        })
        .collect();
    let mut input = serde_json::Map::new();
    input.insert("questions".into(), Value::Array(mapped));
    if let Some(ctx) = context {
        input.insert("context".into(), json!(ctx.fallback_text()));
    }
    Value::Object(input)
}

/// Map the consumer's answers object (`{ "<question text>": "<label>" }`, with
/// multi-select labels joined by `", "`) back to Hope Agent's answer rows.
/// Questions match by text, options by label; a piece matching no option is the
/// user's free-form `custom_input`. Returns `None` when there's nothing usable
/// to inject (e.g. a `{}` cancel/timeout reply), so the tool keeps waiting on
/// the other answer sources rather than injecting an empty answer.
fn map_claude_answers_to_ha(
    questions: &[AskUserQuestion],
    answers: &Value,
) -> Option<Vec<AskUserQuestionAnswer>> {
    let map = answers.as_object()?;
    if map.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    for q in questions {
        let Some(answer_str) = map.get(q.text.fallback_text()).and_then(|v| v.as_str()) else {
            continue;
        };
        let pieces: Vec<&str> = if q.multi_select {
            answer_str
                .split(", ")
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect()
        } else {
            let t = answer_str.trim();
            if t.is_empty() {
                Vec::new()
            } else {
                vec![t]
            }
        };
        let mut selected = Vec::new();
        let mut custom_input: Option<String> = None;
        for piece in pieces {
            match q.options.iter().find(|o| o.label.fallback_text() == piece) {
                Some(opt) => selected.push(opt.value.clone()),
                // A piece matching no option label → free-form custom answer.
                None => custom_input = Some(piece.to_string()),
            }
        }
        out.push(AskUserQuestionAnswer {
            question_id: q.question_id.clone(),
            selected,
            custom_input,
        });
    }
    (!out.is_empty()).then_some(out)
}

#[cfg(test)]
mod hopet_mapping_tests {
    use super::*;
    use crate::ask_user::AskUserQuestionOption;

    fn opt(value: &str, label: &str) -> AskUserQuestionOption {
        AskUserQuestionOption {
            value: value.into(),
            label: AskUserText::plain(label),
            description: None,
            recommended: false,
            preview: None,
            preview_kind: None,
        }
    }

    fn question(
        id: &str,
        text: &str,
        multi: bool,
        options: Vec<AskUserQuestionOption>,
    ) -> AskUserQuestion {
        AskUserQuestion {
            question_id: id.into(),
            text: AskUserText::plain(text),
            options,
            allow_custom: true,
            multi_select: multi,
            template: None,
            header: None,
            timeout_secs: None,
            default_values: Vec::new(),
        }
    }

    #[test]
    fn builds_claude_shape_with_label_and_multiselect() {
        let qs = vec![question(
            "q_drink",
            "喝点什么？",
            false,
            vec![opt("tea", "茶"), opt("coffee", "咖啡")],
        )];
        let v = build_claude_ask_user_tool_input(&qs, Some(&AskUserText::plain("随便选")));
        assert_eq!(v["context"], "随便选");
        let q0 = &v["questions"][0];
        assert_eq!(q0["question"], "喝点什么？");
        assert_eq!(q0["multiSelect"], false);
        // Claude option label is the human text (not the canonical value).
        assert_eq!(q0["options"][0]["label"], "茶");
        assert_eq!(q0["options"][1]["label"], "咖啡");
    }

    #[test]
    fn single_select_label_maps_to_value() {
        let qs = vec![question(
            "q_drink",
            "喝点什么？",
            false,
            vec![opt("tea", "茶"), opt("coffee", "咖啡")],
        )];
        let answers = json!({ "喝点什么？": "咖啡" });
        let mapped = map_claude_answers_to_ha(&qs, &answers).unwrap();
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].question_id, "q_drink");
        assert_eq!(mapped[0].selected, vec!["coffee".to_string()]);
        assert!(mapped[0].custom_input.is_none());
    }

    #[test]
    fn multi_select_splits_on_comma_space() {
        let qs = vec![question(
            "q_relax",
            "放松？",
            true,
            vec![
                opt("music", "听歌"),
                opt("walk", "散步"),
                opt("video", "刷视频"),
            ],
        )];
        let answers = json!({ "放松？": "听歌, 刷视频" });
        let mapped = map_claude_answers_to_ha(&qs, &answers).unwrap();
        assert_eq!(
            mapped[0].selected,
            vec!["music".to_string(), "video".to_string()]
        );
    }

    #[test]
    fn unmatched_label_becomes_custom_input() {
        let qs = vec![question(
            "q_drink",
            "喝点什么？",
            false,
            vec![opt("tea", "茶")],
        )];
        let answers = json!({ "喝点什么？": "气泡水" });
        let mapped = map_claude_answers_to_ha(&qs, &answers).unwrap();
        assert!(mapped[0].selected.is_empty());
        assert_eq!(mapped[0].custom_input.as_deref(), Some("气泡水"));
    }

    #[test]
    fn empty_or_missing_answers_yield_none() {
        let qs = vec![question(
            "q_drink",
            "喝点什么？",
            false,
            vec![opt("tea", "茶")],
        )];
        // `{}` (cancel/timeout) → None, so nothing is injected.
        assert!(map_claude_answers_to_ha(&qs, &json!({})).is_none());
        // An answers map with no matching question text → None.
        assert!(map_claude_answers_to_ha(&qs, &json!({ "别的问题": "x" })).is_none());
    }
}
