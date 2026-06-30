# Phase 2 完整目标与验收清单

> 返回 [Phase 2 Coding Mode 与 Script-first Dynamic Workflow 方案](phase2-coding-mode-dynamic-workflow.md)
>
> 更新时间：2026-06-30
>
> 状态：实现收口清单。5 个无 LLM eval 场景已通过；真实模型回复型 fan-out 作为人工 smoke/eval gate 记录。

## 完整目标

Phase 2 不是只加一个 `/loop` 开关，而是要把 coding 长任务变成可恢复、可观察、可审批、可验证的闭环：

1. Hope-native coding skills 可作为核心策略，不依赖第三方移植 skills。
2. Script-first workflow runtime 可执行模型生成的动态脚本，并通过 Script Gate / permission preview / approval 审核。
3. workflow op 身份由 runtime 位置化 op-key 决定，模型不手写稳定 id。
4. workflow 可编排 `task`、`fileSearch/read/grep/tool`、`spawnAgent/waitAll`、`validate`、`diff`、`askUser`、`trace`、`finish`。
5. 长任务可恢复、可取消、可暂停/恢复、可查看 trace，且不重复已完成副作用。
6. guarded repair loop 至少具备运行时 stop guard：重复验证失败、连续无有效 diff 进展会停止并记录原因。
7. Workspace UI 能看到 run 状态、任务、trace、validation、agents、失败原因。
8. 至少 5 个 coding eval/smoke 场景有明确证据口径，后续接 Phase 0 baseline 不回归。

## 已落实现证据

| 目标 | 证据 |
| --- | --- |
| durable store/state machine | `workflow_runs` / `workflow_ops` / `workflow_events`，owner API，`WorkflowRunState` 转移与 snapshot |
| runtime foundation | QuickJS/rquickjs 受控执行、runtime deterministic guard、位置化 op-key、Completed replay、Started non-idempotent fail-closed |
| host API MVP | `task.create/update`、`fileSearch`、`tool/read/grep`、`workflow.map`、`spawnAgent/waitAll`、`validate`、`askUser`、`diff`、`trace`、`finish` |
| subagent 桥接 | `workflow.spawnAgent` 预分配 child_handle，经真实 `subagent` 工具落 `subagent_runs` 与 `background_jobs` 投影 |
| validation 桥接 | `workflow.validate` 预分配 async exec job，返回 `{ ok, summary, reason, results }` |
| guarded repair stop guard | 失败 validation 写 `guarded_repair_validation_failed`；重复 fingerprint → `guarded_repair_same_validation_fingerprint`；diff hash 不变 → `guarded_repair_no_effective_diff` |
| `/loop` 持久策略 | `sessions.coding_loop_mode` + Tauri/HTTP owner API + system prompt 注入；`off` 禁用 repair guard |
| Workspace UI | Workflow section 支持 run list/actions、Trace / Validation / Agents tabs、blocked reason 展示 |

## 自动化验证

当前应保留这些无 LLM 证据，避免 CI 依赖外部模型、费用或网络状态：

- `cargo test -p ha-core runtime_guarded_repair -- --nocapture`
  - 覆盖重复验证失败 stop guard。
  - 覆盖无有效 diff 进展 stop guard。
- `cargo test -p ha-core runtime_loop_mode_off_does_not_apply_repair_guard -- --nocapture`
  - 覆盖 `loop_mode=off` 不误拦 validation failure。
- `cargo test -p ha-core runtime_spawn_agent_dispatches_real_subagent_tool_with_preallocated_run_id -- --nocapture`
  - 覆盖 workflow → subagent tool → queue/projection 的真实工具路径。
- `cargo test -p ha-core phase2_eval_feature_workflow_writes_diffs_validates_and_finishes -- --nocapture`
  - 覆盖 write → diff → validate → finish 的 feature implementation 闭环。
- `cargo test -p ha-core phase2_eval_user_approval_pause_resume_cancel_flow -- --nocapture`
  - 覆盖 permission preview approval → pause → resume → cancel 的控制面链路。
- `cargo check -p ha-core --tests`
  - 覆盖 Rust workflow runtime / tests 编译边界。
- `pnpm typecheck`
  - 覆盖 Workspace Workflow tabs 的 TypeScript 边界。

## 5 个 eval / smoke 场景

| 场景 | 自动证据 | 人工 smoke 口径 |
| --- | --- | --- |
| parallel review | `runtime_spawn_agent_dispatches_real_subagent_tool_with_preallocated_run_id` | 用真实 provider 跑一个 script，spawn 2 个 read-only reviewer subagents，`waitAll` 汇总，确认 Agents tab 能看到 run/status |
| debug with failing test | `runtime_guarded_repair_blocks_repeated_validation_failure` | 构造一个失败测试，要求 agent 最小修复、单点验证、失败时写 repair feedback |
| feature implementation | `phase2_eval_feature_workflow_writes_diffs_validates_and_finishes` | 让 agent 生成 feature script，必须有 task、diff、validate、finish，并能通过审批后执行 |
| no-progress repair stop | `runtime_guarded_repair_blocks_no_effective_diff_progress` | 让脚本连续两轮 validation 失败但无 diff 变化，确认 run Blocked 且 UI 显示 stop reason |
| user approval / cancel / resume | `phase2_eval_user_approval_pause_resume_cancel_flow` | Draft script 触发 approval，用户 approve 后运行；运行中 pause/resume/cancel 能反映到 run state |

## 边界

- 真实模型回复型 fan-out 不进普通单测。它依赖 provider key、模型可用性、网络和费用，应该作为人工 smoke/eval gate，而不是 CI gate。
- 自动测试只证明 workflow runtime 与既有子系统的接线：permission、async jobs、subagent queue、task、session DB、Workspace snapshot。
- 若要声明“模型体验对齐 Claude Code workflow 能力”，必须额外完成一次真实 provider smoke，并保存 run snapshot / trace / final 作为验收记录。
