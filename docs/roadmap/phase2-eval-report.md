# Phase 2 Eval 验收报告

> 返回 [Phase 2 完整目标与验收清单](phase2-completion-checklist.md)
>
> 更新时间：2026-06-30
>
> 状态：6 个 Phase 2 回归场景已通过，其中真实模型回复型 fan-out 已用本地 mock provider 自动覆盖；外部真实 provider smoke 只作为体验抽检，不作为 CI gate。

## 验收口径

Phase 2 的自动化 eval 不打外部真实 LLM，原因是 provider key、网络、模型可用性和费用都不适合作为稳定 CI 条件。本报告验证的是 workflow runtime 与既有系统的真实接线：task、permission preview、owner action、async jobs、subagent queue、mock-provider model response、file write、diff、validation、guarded repair、snapshot/UI 数据源。

## 6 个场景结果

| 场景 | 证明目标 | 自动证据 | 本轮结果 |
| --- | --- | --- | --- |
| parallel review | workflow script 可编排 subagent，且不绕过 subagent queue / background job projection | `cargo test -p ha-core runtime_spawn_agent_dispatches_real_subagent_tool_with_preallocated_run_id -- --nocapture` | pass |
| model-response fan-out | workflow 可 fan-out 两个子 Agent，子 Agent 真实穿过 chat engine + OpenAI Chat provider adapter，并由 `waitAll` 汇总完成 | `cargo test -p ha-core phase2_eval_parallel_spawn_agents_complete_with_mock_model_response -- --nocapture` | pass |
| debug with failing test | validation failure 产生结构化 repair feedback，重复失败会停止 | `cargo test -p ha-core runtime_guarded_repair -- --nocapture` 中 `runtime_guarded_repair_blocks_repeated_validation_failure` | pass |
| feature implementation | workflow 可完成 write → diff → validate → finish 的实际 coding 变更闭环 | `cargo test -p ha-core phase2_eval_feature_workflow_writes_diffs_validates_and_finishes -- --nocapture` | pass |
| no-progress repair stop | 连续 validation 失败且 diff hash 不变时必须停止 | `cargo test -p ha-core runtime_guarded_repair -- --nocapture` 中 `runtime_guarded_repair_blocks_no_effective_diff_progress` | pass |
| user approval / cancel / resume | Draft approval gate 与 owner pause/resume/cancel 控制面可串联 | `cargo test -p ha-core phase2_eval_user_approval_pause_resume_cancel_flow -- --nocapture` | pass |

## 额外护栏验证

- `cargo test -p ha-core runtime_execution_mode_off_does_not_apply_repair_guard -- --nocapture`：证明 `/mode off` 不误拦 validation failure。
- `cargo check -p ha-core --tests`：证明 workflow runtime 与测试编译边界。
- `pnpm typecheck`：证明 Workspace Workflow Trace / Validation / Agents tabs 的 TypeScript 边界。

## 结论

Phase 2 的自动化验收已经覆盖 6 个核心场景，满足“长任务可持续稳定运行、体验和性能优先”的实现侧证据要求。

外部真实 provider smoke 仍建议保留为体验抽检：它验证模型自然生成脚本、读 Trace/Agents/Validation 面板、以及最终交互体感是否对齐 Claude Code dynamic workflow；但 runtime 安全边界和 provider adapter 接线已经由本地 mock-provider 自动测试覆盖。
