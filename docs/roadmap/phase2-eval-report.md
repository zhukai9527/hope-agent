# Phase 2 Eval 验收报告

> 返回 [Phase 2 完整目标与验收清单](phase2-completion-checklist.md)
>
> 更新时间：2026-06-30
>
> 状态：5 个 Phase 2 无 LLM 回归场景已通过。真实 provider 回复型 fan-out 仍作为人工 smoke gate，不进入 CI。

## 验收口径

Phase 2 的自动化 eval 不打真实 LLM，原因是 provider key、网络、模型可用性和费用都不适合作为稳定 CI 条件。本报告验证的是 workflow runtime 与既有系统的真实接线：task、permission preview、owner action、async jobs、subagent queue、file write、diff、validation、guarded repair、snapshot/UI 数据源。

## 5 个场景结果

| 场景 | 证明目标 | 自动证据 | 本轮结果 |
| --- | --- | --- | --- |
| parallel review | workflow script 可编排 subagent，且不绕过 subagent queue / background job projection | `cargo test -p ha-core runtime_spawn_agent_dispatches_real_subagent_tool_with_preallocated_run_id -- --nocapture` | pass |
| debug with failing test | validation failure 产生结构化 repair feedback，重复失败会停止 | `cargo test -p ha-core runtime_guarded_repair -- --nocapture` 中 `runtime_guarded_repair_blocks_repeated_validation_failure` | pass |
| feature implementation | workflow 可完成 write → diff → validate → finish 的实际 coding 变更闭环 | `cargo test -p ha-core phase2_eval_feature_workflow_writes_diffs_validates_and_finishes -- --nocapture` | pass |
| no-progress repair stop | 连续 validation 失败且 diff hash 不变时必须停止 | `cargo test -p ha-core runtime_guarded_repair -- --nocapture` 中 `runtime_guarded_repair_blocks_no_effective_diff_progress` | pass |
| user approval / cancel / resume | Draft approval gate 与 owner pause/resume/cancel 控制面可串联 | `cargo test -p ha-core phase2_eval_user_approval_pause_resume_cancel_flow -- --nocapture` | pass |

## 额外护栏验证

- `cargo test -p ha-core runtime_loop_mode_off_does_not_apply_repair_guard -- --nocapture`：证明 `/loop off` 不误拦 validation failure。
- `cargo check -p ha-core --tests`：证明 workflow runtime 与测试编译边界。
- `pnpm typecheck`：证明 Workspace Workflow Trace / Validation / Agents tabs 的 TypeScript 边界。

## 结论

Phase 2 的无 LLM 自动化验收已经覆盖 5 个核心场景，满足“长任务可持续稳定运行、体验和性能优先”的实现侧证据要求。

仍保留的人工 gate 只有一项：真实 provider 回复型 subagent fan-out。它用于验证模型体验是否对齐 Claude Code dynamic workflow，而不是验证 runtime 安全边界；执行时需保存 run snapshot、trace、Agents tab 状态和 final 输出作为人工记录。
