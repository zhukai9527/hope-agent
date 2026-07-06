# Loop 控制平面

> 返回 [文档索引](../README.md) | 更新时间：2026-07-06

## 概述

Loop 是通用持续触发控制面：它只表示按时间、条件或后续事件重复触发，不表示执行强度。执行强度仍由 `/mode` / Execution Mode 控制；具体执行编排仍由 Workflow 承载；最终目标和完成标准仍由 Goal 承载。

Loop v2 在 v1 的“能触发”之上补齐可靠治理：复用现有 Cron 调度器，不另起 scheduler；每个 Loop schedule 都有一个受控 Cron job。Cron 负责可靠 tick、primary-only、并发上限、启动恢复、底层失败退避、无人值守权限面；Loop store 负责 session 归属、Goal / Goal criteria 绑定、执行策略、预算、次数、progress ledger、no-progress / failure streak、backoff / blocked 决策和可审计 trace。

## 用户语义

```text
/loop every 10m: check CI and continue fixing if failing
/loop every 10m --workflow: refresh the research brief
/loop until CI is green every 5m: inspect CI and fix the next failing issue
/loop status
/loop status <id>
/loop pause <id>
/loop resume <id>
/loop stop <id>
```

支持的预算参数：

- `--max-runs N`：最多触发次数。
- `--max-runtime 2h`：Loop 从创建开始的最长运行窗口。
- `--tokens N`：Loop token 预算，触发前 hard stop。
- `--workflow` / `--strategy workflow`：仅用于 `every` interval loop；触发时创建并启动绑定 Goal 的 Domain Workflow run，而不是继续原会话。
- `--cost-micros` / `cost_budget_micros` 字段已预留；当前创建时会拒绝该预算，等待 provider cost ledger 接入后放开。

创建 Loop 必须满足二者之一：绑定当前 open/pending closure Goal，或提供明确 recurring prompt。无痕会话拒绝持久化 Loop。

GUI 创建器在 active Goal 有拆分标准时提供「推进标准」选择器；默认绑定整个 Goal，选择具体标准后写 `goalCriterionId`，用于解释这个 Loop 为什么存在、推进哪条完成标准。Slash `/loop` 当前仍只表达 recurring prompt / workflow strategy，不解析 criteria id。

## 数据模型

Loop 表落在 `sessions.db`，随 session 生命周期级联删除。

```text
loop_schedules
  id
  session_id
  goal_id?
  goal_criterion_id?
  goal_criterion_text?
  goal_criterion_kind?
  goal_revision?
  cron_job_id
  prompt
  trigger_kind: interval | cron | condition | event
  trigger_spec_json
  execution_strategy: continue | workflow
  state: active | paused | completed | cancelled | blocked
  max_runs
  run_count
  max_runtime_secs
  token_budget
  cost_budget_micros
  progress_state: progressed | weak_progress | no_progress | blocked | failed | awaiting_approval
  progress_summary
  no_progress_streak
  failure_streak
  max_no_progress_runs
  max_failures
  backoff_secs
  approval_policy_snapshot_json
  created_at / updated_at / completed_at
  blocked_reason
  next_run_at?       # 从 Cron job 派生给 owner API / GUI
  cron_status?       # 从 Cron job 派生给 owner API / GUI

loop_runs
  id
  loop_id
  cron_job_id
  cron_run_log_id?
  session_id
  seq
  state: running | queued | injected | succeeded | empty | failed | cancelled | skipped
  trigger_reason
  result_summary
  error
  progress_state
  progress_delta_json
  no_progress_reason
  scheduling_decision
  trace_json
  started_at / finished_at
```

`execution_strategy`：

- `continue`：默认策略。Cron tick 注入 `<loop_trigger>` 到原会话，触发一次 parent-session continuation。
- `workflow`：interval loop 的受控策略。Cron tick 不注入聊天，而是读取绑定 Goal 的 `workflow_template_id/version/task_type`，生成 Domain Workflow draft，创建并启动 durable Workflow run。

Criteria 绑定：

- `create_loop_schedule` 可接收 `goalCriterionId`；后端校验它属于绑定 Goal 当前 revision，写入 `goal_criterion_id/text/kind/goal_revision`。
- 触发前会重新检查 Goal state 与 criteria revision：Goal completed 会让 Loop completed；Goal failed/cancelled/paused 或 criteria 删除/修改会让 Loop blocked 并暂停 Cron，避免静默推进错误目标。
- Loop 创建、trigger、terminal `loop_run` Goal link metadata 带 `goalCriterion`。
- `execution_strategy=workflow` 派生的 WorkflowRun 继承 `goal_criterion_id`，因此 Goal detail 能按 criteria 同时看到 Loop 与 Workflow 进展。
- Cron 注入的 `<loop_trigger>` 会包含 `<goal_criterion_id>` 与 `<goal_criterion_text>`，让模型在继续会话模式下也知道本轮优先推进哪条标准。

Cron job 的 `CronPayload::SessionLoop` 保存 `loop_id`、原会话 `session_id`、prompt、agent、goal。真实执行策略以 `loop_schedules.execution_strategy` 为准，普通 Cron `AgentTurn` 路径不变。

## 执行链

```mermaid
sequenceDiagram
    participant User
    participant Slash as /loop
    participant Loop as loop_control
    participant Cron as Cron Scheduler
    participant Inject as Parent Injection
    participant Chat as Chat Engine
    participant Workflow as Workflow Runtime

    User->>Slash: /loop every 10m: prompt
    Slash->>Loop: create_loop_schedule
    Loop->>Cron: create CronJob(SessionLoop)
    Cron-->>Loop: cron_job_id
    Loop-->>User: loop id / status

    Cron->>Loop: prepare_loop_cron_run
    Loop-->>Cron: admit / reject
    alt executionStrategy = continue
        Cron->>Inject: inject loop trigger into original session
        Inject->>Chat: run parent turn after idle gate
        Chat-->>Inject: persisted assistant turn
    else executionStrategy = workflow
        Cron->>Workflow: preview domain workflow + create WorkflowRun
        Workflow-->>Cron: run id / primary launch accepted
    end
    Cron->>Loop: finish_loop_cron_run
    Loop-->>User: loop:changed event / Workspace refresh
```

关键点：

- Cron claim 仍是 slot-before-claim，并发上限和 primary-only 语义不变。
- `SessionLoop` 不创建隔离 cron 会话，而是通过 `subagent::injection::inject_and_run_parent` 回到原会话。
- 注入消息带 `<loop_trigger>` 信封，并写 `attachments_meta.loop_trigger`，前端可以识别为系统触发。
- `condition` loop 注入时会带 `<condition>`，并要求 assistant 在条件满足时用 `LOOP_CONDITION_SATISFIED: <reason>` 开头；`finish_loop_cron_run` 识别该 marker 后把 Loop 置 `completed` 并暂停 Cron。
- `workflow` strategy 只支持 `interval` loop。`condition` loop 的完成语义当前依赖 assistant marker，不能伪装成 workflow 完成；后续若要支持，必须等 Workflow terminal event 能反写 condition result。
- `workflow` strategy 必须绑定 Goal，且该 Goal 必须选择 Domain Workflow template。触发时调用 `preview_domain_workflow(requirePlanConfirmation=false)`，通过 Script Gate / permission preview 后创建 `origin=loop:<loop_id>` 的 WorkflowRun 并请求 Primary runtime 启动。
- Loop workflow trigger 的 `loop_runs.trace_json` 会记录 `executionStrategy`、`workflowRunId`、`workflowKind`、`executionMode`、`templateId/version` 和是否需要审批；Workflow run 自己继续拥有完整 ops/events/recovery trace。
- 派生 WorkflowRun 终态后会被 Domain Operational Gate 与 Soak Report 作为同一 session/domain 的长任务运行证据读取；`loop_runs.trace_json.workflowRunId` 是从 Loop run 回到 Workflow detail 的审计索引。
- 若父会话正忙，注入沿用现有 idle gate；若被用户新 turn 抢占，进入 injection queue。
- `loop_schedules.state != active`、达到 `max_runs`、超过 `max_runtime_secs`、Loop token budget exhausted、Goal budget exhausted、Goal terminal、criteria stale 都会在触发前拒绝，并暂停背后的 Cron job。
- run 结束后会计算 deterministic Progress Guard：优先读取 Goal durable evidence delta（workflow completed、validation passed、file changed、artifact created、source cited、domain quality passed 等 strong evidence），再看 Workflow trace / run state；不会把“Loop 跑了一次”本身当成进展。
- `progressed` / `weak_progress` 会清空 no-progress / failure streak；`no_progress` 连续累计后先 backoff，达到 `max_no_progress_runs` 后 blocked；`failed` 连续累计后按 `max_failures` backoff / blocked；`blocked` 立即暂停。
- backoff 通过 CronDB 的窄接口只推迟 active job 的 `next_run_at`，不改变原始 schedule，不复活 paused / terminal job。

## Goal / Workflow / Mode 边界

| 概念 | 职责 |
| --- | --- |
| Goal | 顶层目标、完成标准、证据链、budget hard stop |
| Workflow | 一次具体执行编排，负责 op trace、审批、恢复、验证 |
| Execution Mode | 后续 turn 的推进策略，控制观察/计划/验证/修复强度 |
| Loop | 按时间/条件重复触发下一次推进 |
| Cron | 底层可靠调度器 |

Loop 绑定 Goal 时，每次 run 会写 `loop_run` evidence 到 Goal link/timeline。Loop 不绕过 Goal budget：触发前会调用 Goal budget 门禁，耗尽后 Loop 进入 `blocked` 并暂停 Cron。

Loop 自身 token budget 也会在触发前按 parent session 自创建后的消息 usage 计算；达到上限后进入 `blocked` 并暂停 Cron。成本预算目前只保留字段，不接受创建，避免没有 cost ledger 时给用户错误安全感。

## API / GUI

Owner API：

| Tauri Command | HTTP |
| --- | --- |
| `list_loop_schedules` | `GET /api/sessions/{sessionId}/loops` |
| `create_loop_schedule` | `POST /api/sessions/{sessionId}/loops` |
| `get_loop_schedule` | `GET /api/loops/{loopId}` |
| `pause_loop_schedule` | `POST /api/loops/{loopId}/pause` |
| `resume_loop_schedule` | `POST /api/loops/{loopId}/resume` |
| `stop_loop_schedule` | `POST /api/loops/{loopId}/stop` |
| `run_loop_schedule_now` | `POST /api/loops/{loopId}/run-now` |
| `update_loop_schedule_policy` | `PATCH /api/loops/{loopId}/policy` |

`create_loop_schedule` 额外接受 `executionStrategy?: "continue" | "workflow"`；省略时为 `continue`。Loop v2 还接受 `maxNoProgressRuns`、`maxFailures`、`backoffSecs`；省略时分别为 3 / 3 / 300s。`list_loop_schedules` 与 `get_loop_schedule` 会从 Cron job 派生 `nextRunAt` / `cronStatus`，前端不自行猜算下一次运行。

`run_loop_schedule_now` 复用 Cron 的 `execute_job_public` / primary-only / immediate claim 路径，属于一次性手动触发，不改写 recurring schedule。`update_loop_schedule_policy` 更新 max runs、runtime、token budget、no-progress/failure/backoff 策略，并同步底层 Cron job 的 `max_failures` 与 `job_timeout_secs`；编辑 blocked Loop 的策略会清空当前 no-progress / failure streak，便于用户恢复。

Slash：`/loop every <duration> --workflow: <prompt>` 与 `/loop every <duration> --strategy workflow: <prompt>` 会创建 `executionStrategy=workflow` 的 interval loop；`/loop until ... --workflow` 当前会被拒绝，直到 Workflow terminal event 能反写 condition result。`/loop status` 会展示每个 schedule 的 strategy；`/loop status <id>` 的 Recent runs 会从 `loop_runs.trace_json` 展示派生 workflow run id、template version 和结果摘要。

GUI：Workspace 面板中的 Loop Center 支持创建 `every` / `until` loop，填写 interval、condition、prompt、max runs、max runtime、token budget、no-progress 上限、failure 上限和 backoff 间隔；创建 `every` loop 且当前 active Goal 已选择领域模板时，用户可把执行方式从“继续会话”切到“创建工作流”。

Loop Center 按 blocked / active / paused / completed / cancelled 排序分组，超过 5 个时提供“查看更多 Loop”，不依赖 `/loop status` 完成管理。每行显示状态、progress state、strategy、bound criteria、trigger spec、prompt、run count、next run、runtime / token budget、guard streak、progress summary、blocked reason，并提供 run now / edit policy / history / pause / resume / stop。edit policy 内联编辑 max runs、runtime、token、no-progress、failure、backoff；run now 走 Cron primary-only immediate path。每个 Loop 行可展开“运行记录”，通过 `get_loop_schedule` 拉取最近 `loop_runs`，显示 run seq、state、progress state、调度决策、no-progress reason、错误/摘要、派生 `workflowRunId` 与 template version。

创建 `executionStrategy=workflow` 的 Loop 后，列表会用 `Workflow` 标记，并根据同会话 Workflow run 的 `origin=loop:<loop_id>` 显示最近派生 run 的 kind、state、更新时间和跳转按钮。点击后 Workspace 会选中对应 Workflow run detail，继续查看审批、trace、validation、agents、pause/resume/cancel 等控制面。Workspace 顶层共享同一份 `useGoal`、`useWorkflowRuns` 与 `useLoopSchedules` state 给 Workflow 与 Loop 区块，避免重复请求并确保 active Goal 模板、派生 Workflow run 和 Loop 状态一致；readiness 卡片请求创建 loop 时，Loop 区块会展开创建器并预选 `every + workflow`，但仍由用户显式点击“创建 Loop”。当 readiness 卡片发现 blocked loop 时，“查看 Loop”会展开 Loop 区块并打开对应运行记录；它不自动 resume / stop。

## 安全与可靠性

- 无痕会话拒绝 durable Loop。
- Loop 不新增工具权限捷径；实际 turn 仍走原会话的 permission mode、sandbox、hooks、Project/KB access。
- Loop 背后的 `CronPayload::SessionLoop` 是受控 Cron job；模型侧 `manage_cron` 不能 update / pause / resume / delete，必须走 Loop 控制面，避免 Loop store 与 Cron 状态分叉。
- Cron 背景无人值守语义保持 fail-closed 或遵循显式 policy。
- Loop workflow strategy 不插入 `workflow.askUser` 计划确认；自动触发不能自己制造无人值守确认死锁。敏感动作仍由 Workflow permission preview、运行时权限引擎、Domain Quality approval gate 和连接器授权 fail-closed。
- Loop workflow trigger 不绕过 Workflow Script Gate；内置 Domain Workflow draft 必须包含 task truth、`workflow.finish`、`workflow.verify` 复核计划和显式 budget hint。
- Loop 停止只把 Loop 置 `cancelled` 并暂停底层 Cron job；不会删除历史 trace。
- EventBus 发 `loop:changed`，前端和 HTTP/WS 订阅可刷新状态。

## Loop v2 已落地能力与后续边界

Loop v2 当前已把 Loop 从“能触发”升级为可靠、可治理、可解释的持续推进器：

- Loop Center 能在 GUI 内完成核心管理：分组、查看更多、run detail、next run、progress state、guard streak、budget、blocked reason、run now、edit policy、pause/resume/stop。
- Progress Guard 已基于 deterministic durable evidence delta：strong evidence 包括 workflow completed、validation passed、review passed、domain quality passed、task completed、diff/file/artifact/source/data-quality/user-decision 等；弱信号和 no-progress 会分开记录。
- 连续 `no_progress` / `failed` 会 backoff；达到上限会 blocked 并暂停 Cron；blocked reason 和 run history 可见。
- Goal completed 会让绑定 Loop completed；Goal failed/cancelled/paused、Goal criteria 删除或 revision/text/kind 变更会让 Loop blocked，要求用户重新确认或编辑策略。
- `run_loop_schedule_now` 复用 Cron immediate path；`update_loop_schedule_policy` 同步 Loop store 与 Cron guard，避免双状态分叉。
- Workflow strategy 的 Loop run 记录 `workflowRunId` / template version，GUI 可从 Loop 跳到 Workflow detail；Workflow run `origin=loop:<id>` 可反向聚合到 Loop 行。

仍保持的保守边界：

- Loop 仍只表示持续触发器，不重新承载执行强度；执行强度继续归 `/mode` / Execution Mode，具体执行继续归 Workflow。
- Slash `/loop` 仍保持简单；v2 guard 策略主要在 GUI / owner API 暴露，slash 不解析 criteria id / policy edit。
- Event-triggered Loop 仍是后续池。当前 `trigger_kind=event` 是数据模型预留，但创建时仍拒绝；先不接外部 webhook / file watcher / CI provider / connector object stream，避免引入未治理的事件风暴。
- Condition workflow 仍等待 Workflow terminal event 能反写 condition result 后再放开；当前 `until` loop 继续依赖 conversation continuation + assistant marker，不能伪装成 workflow 完成。
- 成本预算精确统计仍等待 provider cost ledger；在此之前 `cost_budget_micros` 继续保持保守拒绝，避免给用户错误安全感。

这些边界保证后续增强不推翻当前契约：Loop 管触发、progress guard 和调度治理，不拥有 Goal 完成标准，也不绕过 Workflow、权限、预算和无痕红线。

## 测试覆盖

- `workflow_strategy_materializes_domain_workflow_run` 覆盖 Goal 绑定领域模板后，interval Loop workflow strategy 能生成 `origin=loop:<id>` 的 durable WorkflowRun，并把 `workflowRunId` / template version 写入 loop run trace。
- `workflow_strategy_feeds_operational_and_soak_gates` 覆盖同一条 Goal → Loop tick → WorkflowRun → terminal → LoopRun trace 链路会进入 Domain Operational Gate 和 Soak Report，证明 Workspace 的运行稳定性 / 长跑审计卡片读取的是真实控制面证据。
- `no_progress_backoff_then_blocks_after_threshold` 覆盖连续无进展先 backoff、再 blocked。
- `durable_goal_evidence_resets_no_progress_streak` 覆盖 strong Goal evidence 会把 progress 判为 `progressed` 并清空空转 streak。
- `goal_completed_stops_bound_loop_before_next_trigger` 覆盖绑定 Goal completed 后 Loop 自动 completed。
- `criteria_revision_change_blocks_loop_until_rebind` 覆盖 Goal criteria 修改后 Loop blocked。
- `loop_policy_update_persists_budget_and_cron_guard` 覆盖策略编辑会同时更新 Loop store 与 Cron job。
- `WorkspacePanel` Loop 相关 Vitest 覆盖 derived workflow 行、run history、Loop Center view-more、run-now 和 policy edit。
