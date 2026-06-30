use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context as _, Result};
use rquickjs::prelude::{Func, MutFn};
use rquickjs::{
    CatchResultExt, Context, Ctx, Exception, Function, Object, Runtime, Value as JsValue,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::plan::{check_workflow_script_draft, ScriptGateOptions};
use crate::session::{SessionDB, Task, TaskStatus};

use super::types::{
    UpsertWorkflowOpInput, WorkflowEffectClass, WorkflowOpState, WorkflowRunSnapshot,
    WorkflowRunState,
};

const DEFAULT_SCRIPT_TIMEOUT_SECS: u64 = 30;
const MAX_SCRIPT_TIMEOUT_SECS: u64 = 300;
const SCRIPT_MEMORY_LIMIT_BYTES: usize = 64 * 1024 * 1024;
const SCRIPT_STACK_LIMIT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRuntimeResult {
    pub snapshot: WorkflowRunSnapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
}

pub fn run_workflow_script(db: Arc<SessionDB>, run_id: &str) -> Result<WorkflowRuntimeResult> {
    let run = db
        .get_workflow_run(run_id)?
        .ok_or_else(|| anyhow!("workflow run {} not found", run_id))?;

    if run.state == WorkflowRunState::Completed {
        return Ok(WorkflowRuntimeResult {
            snapshot: db
                .workflow_run_snapshot(run_id, 500)?
                .ok_or_else(|| anyhow!("workflow run {} not found", run_id))?,
            output: None,
        });
    }
    if matches!(
        run.state,
        WorkflowRunState::Failed | WorkflowRunState::Cancelled | WorkflowRunState::Blocked
    ) {
        return Err(anyhow!(
            "workflow run {} is terminal ({}); refusing to execute script",
            run_id,
            run.state.as_str()
        ));
    }
    if run.state == WorkflowRunState::Paused {
        return Err(anyhow!("workflow run {} is paused", run_id));
    }

    let gate = check_workflow_script_draft(&run.script_source, ScriptGateOptions::default());
    if !gate.passed() {
        return Err(anyhow!(gate.render_feedback("Workflow Script Gate")));
    }

    if run.state != WorkflowRunState::Running {
        db.transition_workflow_run(run_id, WorkflowRunState::Running, Some("runtime_start"))?;
    }

    let output = match execute_script(db.clone(), &run) {
        Ok(output) => output,
        Err(err) => {
            let _ =
                db.transition_workflow_run(run_id, WorkflowRunState::Failed, Some("runtime_error"));
            return Err(err);
        }
    };

    let snapshot = db
        .workflow_run_snapshot(run_id, 500)?
        .ok_or_else(|| anyhow!("workflow run {} not found", run_id))?;
    Ok(WorkflowRuntimeResult {
        snapshot,
        output: Some(output),
    })
}

fn execute_script(db: Arc<SessionDB>, run: &super::types::WorkflowRun) -> Result<Value> {
    let runtime = Runtime::new().context("create QuickJS runtime")?;
    runtime.set_memory_limit(SCRIPT_MEMORY_LIMIT_BYTES);
    runtime.set_max_stack_size(SCRIPT_STACK_LIMIT_BYTES);

    let timeout = script_timeout(run);
    let started_at = Instant::now();
    runtime.set_interrupt_handler(Some(Box::new(move || started_at.elapsed() >= timeout)));

    let ctx = Context::full(&runtime).context("create QuickJS context")?;
    ctx.with(|ctx| -> Result<Value> {
        let host = Rc::new(RefCell::new(WorkflowRuntimeHost::new(
            db.clone(),
            run.id.clone(),
            run.session_id.clone(),
        )));
        let workflow = build_workflow_object(ctx.clone(), host.clone())?;
        ctx.globals()
            .set("workflow", workflow.clone())
            .context("install workflow global")?;
        install_runtime_guards(&ctx)?;

        let script = prepare_script_for_eval(&run.script_source);
        ctx.eval::<(), _>(script)
            .catch(&ctx)
            .map_err(|err| anyhow!("workflow script load failed: {}", err))?;

        let main: Function = ctx
            .globals()
            .get("__hopeWorkflowMain")
            .context("workflow script must export default function main(workflow)")?;
        let raw = main
            .call::<_, JsValue>((workflow,))
            .catch(&ctx)
            .map_err(|err| anyhow!("workflow script failed: {}", err))?;
        let _returned = finish_maybe_promise(ctx.clone(), raw)
            .map_err(|err| anyhow!("workflow script promise failed: {}", err))?;

        let finished = host
            .borrow()
            .finished_output
            .clone()
            .ok_or_else(|| anyhow!("workflow script exited without workflow.finish(result)"))?;
        Ok(finished)
    })
}

fn build_workflow_object<'js>(
    ctx: Ctx<'js>,
    host: Rc<RefCell<WorkflowRuntimeHost>>,
) -> rquickjs::Result<Object<'js>> {
    let workflow = Object::new(ctx.clone())?;
    let task = Object::new(ctx.clone())?;

    let create_host = host.clone();
    task.set(
        "create",
        Func::from(MutFn::from(
            move |ctx: Ctx<'js>, args: JsValue<'js>| -> rquickjs::Result<JsValue<'js>> {
                host_call(&ctx, &create_host, args, WorkflowRuntimeHost::task_create)
            },
        )),
    )?;

    let update_host = host.clone();
    task.set(
        "update",
        Func::from(MutFn::from(
            move |ctx: Ctx<'js>, args: JsValue<'js>| -> rquickjs::Result<JsValue<'js>> {
                host_call(&ctx, &update_host, args, WorkflowRuntimeHost::task_update)
            },
        )),
    )?;
    workflow.set("task", task)?;

    let file_search_host = host.clone();
    workflow.set(
        "fileSearch",
        Func::from(MutFn::from(
            move |ctx: Ctx<'js>, args: JsValue<'js>| -> rquickjs::Result<JsValue<'js>> {
                host_call(
                    &ctx,
                    &file_search_host,
                    args,
                    WorkflowRuntimeHost::file_search,
                )
            },
        )),
    )?;

    let trace_host = host.clone();
    workflow.set(
        "trace",
        Func::from(MutFn::from(
            move |ctx: Ctx<'js>, args: JsValue<'js>| -> rquickjs::Result<JsValue<'js>> {
                host_call(&ctx, &trace_host, args, WorkflowRuntimeHost::trace)
            },
        )),
    )?;

    let finish_host = host.clone();
    workflow.set(
        "finish",
        Func::from(MutFn::from(
            move |ctx: Ctx<'js>, args: JsValue<'js>| -> rquickjs::Result<JsValue<'js>> {
                host_call(&ctx, &finish_host, args, WorkflowRuntimeHost::finish)
            },
        )),
    )?;

    Ok(workflow)
}

fn install_runtime_guards(ctx: &Ctx<'_>) -> Result<()> {
    ctx.eval::<(), _>(
        r#"
        const __HopeNativeDate = Date;
        function __hopeDeterminismError(name) {
          throw new Error(`${name} is disabled in workflow runtime; use a workflow host API deterministic source instead`);
        }
        function HopeWorkflowDate(...args) {
          if (args.length === 0) {
            __hopeDeterminismError("new Date()");
          }
          if (new.target) {
            return Reflect.construct(__HopeNativeDate, args, new.target);
          }
          return __HopeNativeDate(...args);
        }
        Object.setPrototypeOf(HopeWorkflowDate, __HopeNativeDate);
        HopeWorkflowDate.prototype = __HopeNativeDate.prototype;
        HopeWorkflowDate.now = () => __hopeDeterminismError("Date.now()");
        HopeWorkflowDate.parse = __HopeNativeDate.parse;
        HopeWorkflowDate.UTC = __HopeNativeDate.UTC;
        globalThis.Date = HopeWorkflowDate;
        Math.random = () => __hopeDeterminismError("Math.random()");
        "#,
    )
    .catch(ctx)
    .map_err(|err| anyhow!("install workflow runtime guards failed: {}", err))
}

fn host_call<'js>(
    ctx: &Ctx<'js>,
    host: &Rc<RefCell<WorkflowRuntimeHost>>,
    args: JsValue<'js>,
    f: fn(&mut WorkflowRuntimeHost, Value) -> Result<Value>,
) -> rquickjs::Result<JsValue<'js>> {
    let args = js_to_json(ctx, args)?;
    let output = host
        .try_borrow_mut()
        .map_err(|_| Exception::throw_message(ctx, "workflow host API called recursively"))?
        .call(args, f)
        .map_err(|err| js_error(ctx, err))?;
    json_to_js(ctx.clone(), output)
}

fn js_to_json<'js>(ctx: &Ctx<'js>, value: JsValue<'js>) -> rquickjs::Result<Value> {
    rquickjs_serde::from_value_strict(value)
        .map_err(|err| Exception::throw_message(ctx, &format!("invalid workflow host args: {err}")))
}

fn json_to_js<'js>(ctx: Ctx<'js>, value: Value) -> rquickjs::Result<JsValue<'js>> {
    rquickjs_serde::to_value(ctx.clone(), value)
        .map_err(|err| Exception::throw_message(&ctx, &format!("serialize workflow result: {err}")))
}

fn js_error<'js>(ctx: &Ctx<'js>, err: anyhow::Error) -> rquickjs::Error {
    Exception::throw_message(ctx, &format!("{err:#}"))
}

fn finish_maybe_promise<'js>(
    ctx: Ctx<'js>,
    value: JsValue<'js>,
) -> rquickjs::CaughtResult<'js, JsValue<'js>> {
    if value.is_promise() {
        let promise = value.into_promise().expect("checked promise");
        promise.finish::<JsValue>().catch(&ctx)
    } else {
        Ok(value)
    }
}

fn prepare_script_for_eval(script: &str) -> String {
    let trimmed = script.trim();
    let prepared = if trimmed.contains("export default") {
        trimmed.replacen("export default", "globalThis.__hopeWorkflowMain =", 1)
    } else {
        let mut s = trimmed.to_string();
        if !s.contains("__hopeWorkflowMain") && s.contains("function main") {
            s.push_str("\nglobalThis.__hopeWorkflowMain = main;");
        }
        s
    };
    format!("\"use strict\";\n{prepared}")
}

fn script_timeout(run: &super::types::WorkflowRun) -> Duration {
    let secs = run
        .budget
        .get("maxScriptSecs")
        .or_else(|| run.budget.get("max_script_secs"))
        .or_else(|| run.budget.get("maxRuntimeSecs"))
        .or_else(|| run.budget.get("max_runtime_secs"))
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_SCRIPT_TIMEOUT_SECS)
        .clamp(1, MAX_SCRIPT_TIMEOUT_SECS);
    Duration::from_secs(secs)
}

struct WorkflowRuntimeHost {
    db: Arc<SessionDB>,
    run_id: String,
    session_id: String,
    next_op_index: usize,
    finished_output: Option<Value>,
}

impl WorkflowRuntimeHost {
    fn new(db: Arc<SessionDB>, run_id: String, session_id: String) -> Self {
        Self {
            db,
            run_id,
            session_id,
            next_op_index: 0,
            finished_output: None,
        }
    }

    fn call(
        &mut self,
        args: Value,
        f: fn(&mut WorkflowRuntimeHost, Value) -> Result<Value>,
    ) -> Result<Value> {
        f(self, args)
    }

    fn task_create(&mut self, args: Value) -> Result<Value> {
        let title = required_string(&args, "title")?;
        let label = optional_string(&args, "label");
        let input = compact_input(args);
        self.execute_op(
            "task.create",
            WorkflowEffectClass::Idempotent,
            input,
            |host| {
                let task = host
                    .db
                    .create_task(&host.session_id, &title, None)
                    .context("create workflow task")?;
                let tasks = host.db.list_tasks(&host.session_id).unwrap_or_default();
                crate::session::emit_task_snapshot(&host.session_id, &tasks);
                crate::hooks::fire_task_created(&host.session_id, &title, None, "");
                Ok(task_handle(&task, label.as_deref()))
            },
        )
    }

    fn task_update(&mut self, args: Value) -> Result<Value> {
        let id = task_id_from_args(&args)?;
        let status = optional_string(&args, "status")
            .map(|value| {
                TaskStatus::from_str(&value)
                    .ok_or_else(|| anyhow!("invalid task status '{}'", value))
            })
            .transpose()?;
        let content = optional_string(&args, "title").or_else(|| optional_string(&args, "content"));
        let active_form = optional_string(&args, "activeForm");
        if status.is_none() && content.is_none() && active_form.is_none() {
            return Err(anyhow!(
                "workflow.task.update requires status, title/content, or activeForm"
            ));
        }

        let input = compact_input(args);
        self.execute_op(
            "task.update",
            WorkflowEffectClass::Idempotent,
            input,
            |host| {
                let current = host.db.list_tasks(&host.session_id)?;
                if !current.iter().any(|task| task.id == id) {
                    return Err(anyhow!(
                        "task {} does not belong to workflow session {}",
                        id,
                        host.session_id
                    ));
                }
                let updated =
                    host.db
                        .update_task(id, status, content.as_deref(), active_form.as_deref())?;
                let tasks = host.db.list_tasks(&host.session_id).unwrap_or_default();
                crate::session::emit_task_snapshot(&host.session_id, &tasks);
                if status == Some(TaskStatus::Completed) {
                    crate::hooks::fire_task_completed(&host.session_id, id, &updated.content);
                }
                Ok(task_handle(&updated, None))
            },
        )
    }

    fn file_search(&mut self, args: Value) -> Result<Value> {
        let query = required_string(&args, "query")?;
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .map(|n| n as usize);
        let root = optional_string(&args, "root")
            .or_else(|| workflow_root_for_session(&self.db, &self.session_id))
            .ok_or_else(|| anyhow!("workflow.fileSearch requires a session working directory"))?;
        let input = json!({
            "query": query.clone(),
            "limit": limit,
            "root": root.clone(),
            "label": optional_string(&args, "label"),
        });
        self.execute_op("fileSearch", WorkflowEffectClass::Pure, input, |_host| {
            let response = crate::filesystem::search_files(&root, &query, limit)
                .context("workflow.fileSearch failed")?;
            serde_json::to_value(response).context("serialize fileSearch response")
        })
    }

    fn trace(&mut self, args: Value) -> Result<Value> {
        let label = optional_string(&args, "label");
        let payload = args.get("payload").cloned().unwrap_or(Value::Null);
        let input = compact_input(args);
        self.execute_op("trace", WorkflowEffectClass::Pure, input, |host| {
            let event = host.db.append_workflow_event(
                &host.run_id,
                "trace",
                json!({
                    "label": label,
                    "payload": payload,
                }),
            )?;
            Ok(json!({ "eventSeq": event.seq }))
        })
    }

    fn finish(&mut self, args: Value) -> Result<Value> {
        let output_arg = args.clone();
        let input = compact_input(args);
        let output = self.execute_op("finish", WorkflowEffectClass::Pure, input, |_host| {
            Ok(output_arg)
        })?;
        self.finished_output = Some(output.clone());
        self.db.transition_workflow_run(
            &self.run_id,
            WorkflowRunState::Completed,
            Some("workflow_finish"),
        )?;
        Ok(output)
    }

    fn execute_op<F>(
        &mut self,
        op_type: &str,
        effect_class: WorkflowEffectClass,
        input: Value,
        f: F,
    ) -> Result<Value>
    where
        F: FnOnce(&mut WorkflowRuntimeHost) -> Result<Value>,
    {
        let op_key = self.next_op_key(op_type);
        let op = self.db.upsert_workflow_op_started(UpsertWorkflowOpInput {
            run_id: self.run_id.clone(),
            op_key: op_key.clone(),
            op_type: op_type.to_string(),
            effect_class,
            input,
            child_handle: None,
        })?;

        match op.state {
            WorkflowOpState::Completed => return Ok(op.output.unwrap_or(Value::Null)),
            WorkflowOpState::Failed => {
                return Err(anyhow!(
                    "workflow op {} previously failed: {}",
                    op_key,
                    op.error.unwrap_or(Value::Null)
                ));
            }
            WorkflowOpState::Pending | WorkflowOpState::Started => {}
        }

        let output = match f(self) {
            Ok(output) => output,
            Err(err) => {
                let _ = self.db.fail_workflow_op(
                    &self.run_id,
                    &op_key,
                    json!({ "message": err.to_string() }),
                );
                return Err(err);
            }
        };
        self.db
            .complete_workflow_op(&self.run_id, &op_key, output.clone())?;
        Ok(output)
    }

    fn next_op_key(&mut self, op_type: &str) -> String {
        let idx = self.next_op_index;
        self.next_op_index += 1;
        format!("main/op#{idx}({op_type})")
    }
}

fn task_handle(task: &Task, label: Option<&str>) -> Value {
    json!({
        "id": task.id,
        "sessionId": task.session_id,
        "title": task.content,
        "status": task.status,
        "label": label,
    })
}

fn task_id_from_args(args: &Value) -> Result<i64> {
    let task = args
        .get("task")
        .ok_or_else(|| anyhow!("workflow.task.update requires task handle from task.create"))?;
    if let Some(id) = task.as_i64() {
        return Ok(id);
    }
    if let Some(id) = task.as_str().and_then(|s| s.parse::<i64>().ok()) {
        return Ok(id);
    }
    task.get("id")
        .and_then(Value::as_i64)
        .ok_or_else(|| anyhow!("workflow.task.update task handle must include id"))
}

fn required_string(args: &Value, key: &str) -> Result<String> {
    optional_string(args, key).ok_or_else(|| anyhow!("missing required string field '{}'", key))
}

fn optional_string(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

fn compact_input(value: Value) -> Value {
    value
}

fn workflow_root_for_session(db: &SessionDB, session_id: &str) -> Option<String> {
    match db.get_session(session_id) {
        Ok(Some(meta)) => {
            if let Some(wd) = meta.working_dir.clone().filter(|s| !s.trim().is_empty()) {
                return Some(wd);
            }
            crate::session::effective_working_dir_for_meta(&meta)
        }
        Ok(None) => None,
        Err(err) => {
            crate::app_warn!(
                "workflow",
                "resolve_root",
                "session {} lookup failed while resolving workflow root: {}",
                session_id,
                err
            );
            None
        }
    }
    .or_else(|| {
        std::env::current_dir()
            .ok()
            .map(|p| p.to_string_lossy().to_string())
    })
}
