# Goal v2 路线图

> 返回 [路线图索引](README.md)
>
> 日期：2026-07-05
>
> 状态：Goal v2 核心闭环已落地，roadmap 不归档。G2.1-G2.6 的实现事实与验证已落入 [`docs/architecture/goal.md`](../architecture/goal.md)；后续增强继续在本文的「后续增强池」推进，但不阻塞当前 Goal v2 可用闭环。

## 1. 背景

Agent 控制平面 v1 已关闭。v1 中 Goal 已具备 durable store、completion criteria、budget、evidence、final audit、GUI goal strip、输入框目标模式、Workflow 自动绑定和模型 prompt 感知。

但 v1 的 Goal 仍偏“目标标签 + 证据汇总”。它能告诉系统当前目标是什么，也能在 workflow 结束后做规则审计；但从用户体验看，还没有完全成为一个长期任务的“目标控制台”：

- 用户仍需要猜测目标什么时候真正结束。
- completion criteria 主要是文本，结构化程度不够。
- 目标修改、拆分、关闭、延期、转后续池的流程还不够显式。
- Goal 与 Workflow / Loop / Task / Evidence 的映射还可以更清楚。
- final audit 能判定 blocked/completed，但缺少更强的用户确认、后续池和变更记录体验。

Goal v2 的任务不是加强执行引擎，而是加强 **终点定义、完成判定、目标治理和用户可控性**。

## 2. 产品目标

Goal v2 要回答一个普通用户最关心的问题：

```text
我到底想完成什么？
现在做到哪了？
还差什么？
为什么还没结束？
什么时候可以放心关闭？
关闭后哪些东西进入后续？
```

成功标准：

- 用户不用读聊天历史，也能从 GUI 看懂 active Goal 的目标、完成标准、进度、证据、阻塞项和下一步。
- 模型每一轮都能稳定感知 active Goal、用户修改后的 Goal、关闭条件和未满足证据。
- Goal 能表达“必须完成项 / 可选项 / 后续增强项”，避免把后续增强无限拖进当前目标。
- Goal final audit 能产出可复核 closure packet：完成了什么、证据是什么、未证明什么、用户是否接受。
- Goal 修改后，旧 audit 不会误导模型；变更历史可追溯。
- Goal 能适用于 coding、research、writing、data analysis、meeting prep、inbox、project ops 等通用场景。

## 3. 非目标

Goal v2 不做这些事：

- 不让 Goal 直接执行工具；执行仍由 Workflow / Chat Engine 承担。
- 不把 Loop 的触发语义塞进 Goal。
- 不重做 Workflow runtime、script replay、approval、trace、repair。
- 不新增大量用户配置项；默认策略要足够好。
- 不把所有后续增强都做成当前 Goal 的 blocker。
- 不把 architecture 文档提前写成已实现事实。

## 4. 核心设计原则

| 原则 | 含义 |
| --- | --- |
| Goal 是终点，不是执行步骤 | Goal 只定义 outcome、criteria、evidence 和 closure，不承载 workflow op。 |
| 用户最终确认优先 | 模型可以建议完成，规则可以审计完成，但长期目标关闭必须能表达用户接受与取舍。 |
| 修改即失效旧结论 | 用户修改 objective / criteria / scope 后，旧 final audit 必须清空或降级为历史记录。 |
| 必须项与后续项分离 | 必须项阻塞 Goal；后续项进入 backlog，不继续拖住当前目标。 |
| 证据比聊天更可靠 | 完成判定依赖 workflow/task/evidence/event，而不是从聊天文本里反扫。 |
| 通用场景一等支持 | Goal 不能写死 coding 语义；coding 只是一个 domain。 |

## 5. 用户体验目标

### 5.1 输入框

- `+` 菜单保留“目标模式”入口。
- 进入目标模式后，composer 上方显示“正在设置目标”状态。
- 用户发送的消息等价于 `/goal <objective>`，但消息气泡不展示 `/goal` 字符。
- 目标消息气泡有“目标”标记。
- 若已有 active Goal，用户可选择：
  - 更新当前目标。
  - 新建替代目标并关闭旧目标。
  - 把输入作为当前目标的新增必须完成项。
  - 把输入作为当前目标的新增可选项。
  - 把输入作为不阻塞当前关闭的后续项。

### 5.2 常驻 Goal Strip

输入框上方与 Workspace 都应显示 active Goal：

- objective 摘要。
- 当前状态：active / paused / blocked / evaluating / completed。
- 已满足 / 未满足 criteria 数量。
- 最近一次 evidence 或 blocker。
- 预算状态。
- 操作：编辑、暂停/恢复、评估、关闭、转后续。

### 5.3 Goal Detail

Goal 详情页或 Workspace 详情应包含：

- Objective。
- Completion Criteria，分为 `required` / `optional` / `follow_up`。
- Evidence Map：每条 criteria 绑定哪些 workflow/task/file/artifact/validation/review evidence。
- Timeline：用户创建/修改、workflow run、loop trigger、task 更新、audit 结果。
- Next Evidence Needed：下一步缺什么证据。
- Closure Packet：完成摘要、未证明项、用户接受记录、后续池，并支持复制给外部 reviewer。

## 6. 数据模型增强

Goal v2 优先以兼容扩展实现，不破坏 v1 durable store。

建议新增或扩展：

| 能力 | 设计 |
| --- | --- |
| Structured criteria | 从纯文本 criteria 派生 `criteria_items`：`id`、`text`、`kind(required/optional/follow_up)`、`status`、`evidenceIds`、`lastReason`。 |
| Goal revision | 每次 objective / criteria / domain 修改生成 revision；final audit 后出现新的 Goal evidence link 也会让旧 audit 标记 stale。 |
| Closure decision | 记录用户是否接受当前 audit：`accepted_v1` / `needs_strict_evidence` / `cancelled` / `superseded`。 |
| Follow-up pool | 将非阻塞增强转为 goal-scoped follow-up item，后续可迁移到 roadmap/task。 |
| Goal snapshot prompt | 给模型注入 compact snapshot：objective、required missing、accepted tradeoff、active blockers。 |

数据仍落 `sessions.db`，并继续遵守：

- incognito 不持久化 Goal。
- 同一普通 session 只允许一个 open Goal 或 pending closure Goal。
- owner 平面负责用户可见修改。
- agent 不能绕过 owner 平面直接改 Goal。

## 7. 模型感知

Goal v2 需要把 active Goal 以稳定、低噪音的方式注入模型：

```text
# Active Goal
Objective: ...
State: active
Required criteria:
- [missing] ...
- [satisfied] ... evidence: workflow_run:...
Current blockers:
- ...
User closure preference:
- accepts v1 substitutes / requires strict evidence / unknown
Next evidence needed:
- ...
```

规则：

- Goal snapshot 必须随用户修改实时更新。
- 已完成但未被用户接受的 Goal 不能让模型擅自宣称“目标关闭”。
- 如果 final audit stale，prompt 必须明确 stale reason。
- Prompt 注入应保持紧凑，避免把完整 evidence timeline 塞进每轮上下文。

## 8. 阶段计划

### G2.1 Structured Goal Criteria（已落地）

目标：让完成标准从自由文本升级为可审计项。

工作项：

- criteria parser：把多行文本解析为稳定 criteria item。
- 支持 `required` / `optional` / `follow_up`。
- Goal evaluator 输出逐条 criteria 状态。
- GUI 可编辑 criteria 文本，并在保存前预览 parser 派生出的 `required` / `optional` / `follow_up` item；保存后在 Goal detail 中显示证据绑定。

验收：

- 修改 criteria 或 final audit 后新增 Goal evidence link 时，旧 audit 自动 stale。
- 每条 required criteria 都能显示 satisfied/missing/blocker。
- optional/follow_up 不阻塞 Goal completed。

### G2.2 Goal Revision 与修改闭环（已落地）

目标：用户修改目标后，模型和审计都能感知。

工作项：

- 为 Goal objective / criteria / domain 变更记录 revision。
- final audit 记录对应 revision。
- GUI 显示“目标已修改，需重新评估”。
- 模型 prompt 注入最新 revision 和 stale audit 状态。

验收：

- 更新目标后，旧 completed 结论不会继续显示为当前完成。
- Workflow 新 evidence 绑定到最新 revision。
- 历史 timeline 能看见修改记录。

### G2.3 Goal Control Center（已落地）

目标：把 Goal 从 strip 升级为可操作详情面板。

工作项：

- Workspace 中独立 Goal detail 区块。
- Criteria / Evidence / Timeline / Budget / Closure 五个区域。
- 支持编辑、暂停/恢复、evaluate、close、追加 follow-up、move to follow-up。
- 支持从 evidence 跳到 workflow run、task、artifact、file。

验收：

- 用户不用 slash command 就能完成 Goal 创建、查看、更新、评估和关闭。
- 关闭前能看到未证明项和后续池。
- 窄屏不遮挡 composer，不出现文本溢出。

### G2.4 Final Audit v2 与 Closure Packet（已落地）

目标：让 Goal 关闭有可复核记录。

工作项：

- final audit 输出 closure packet。
- 支持用户选择 `accept_v1` 或 `needs_strict_evidence`。
- 将未证明项转入 follow-up pool。
- 生成可复制 review packet 摘要，并在 Goal detail 直接复制。

验收：

- completed Goal 记录用户接受方式。
- packet 明确哪些已证明、哪些未证明、哪些进入后续。
- 模型不能在缺用户接受时自动关闭长期 Goal。

### G2.5 Goal-aware Workflow / Loop Handoff（已落地）

目标：让 Workflow 和 Loop 更清楚地服务 Goal。

工作项：

- Workflow 创建页/运行详情显示它推进哪条 criteria。
- Loop 创建时选择推进哪条 Goal criteria。
- Goal detail 中按 criteria 聚合 workflow runs / loop runs。
- blocked Goal 可以推荐下一条 workflow 或 loop。

验收：

- 用户能回答“这个 workflow/loop 为什么存在，它推进哪条目标”。
- Goal blocked 时能看到下一步证据建议。
- Loop 空转不会被误判为 Goal 进展。

落地说明：

- `workflow_runs` 写入 `goal_criterion_id/text/kind/goal_revision`；`workflow_run` 模型工具、HTTP/Tauri owner API 和 GUI 创建器都可传 `goalCriterionId`。
- `loop_schedules` 写入同一组 criteria 快照；Loop trigger / run evidence 带 `goalCriterion`，workflow strategy 派生 run 继承 criteria。
- Active Goal prompt 注入 criteria id 列表，模型创建 workflow 时可选择具体 `goalCriterionId`。
- Goal detail criteria 行展示绑定 Workflow / Loop / evidence 数量；Workflow run list/detail 与 Loop list 展示 criteria 标签。
- Workspace 与输入框的 Goal 编辑区都会即时预览 criteria 草稿如何被解释为必须项、可选项和后续项，避免用户把后续增强误写成当前 blocker。
- Goal audit 按 metadata `goalCriterion.id` 优先聚合 evidence，绑定到某条 criteria 的 blocker 不再污染其它 criteria。

### G2.6 Goal v2 完整闭环验证（已落地）

目标：证明 Goal v2 不只是 UI，而是真能改善长期任务闭环。

测试与样本：

- Rust deterministic tests：criteria parser、revision stale、audit gate、closure decision。
- GUI Vitest：目标模式发送、active Goal 下更新 / 替代 / 追加 criteria / 追加 follow-up 的输入路径、常驻 Goal strip、Goal 创建、closure packet、criteria evidence map、后续池、关键 evidence 展示。
- Source-level UX audit：输入框目标模式、Goal strip、Goal detail、closure packet。
- 至少 3 个 domain fixture：coding、research、writing。

本轮已验证：

- `cargo test -p ha-core goal --locked`：42 个 goal 相关匹配测试通过，覆盖 structured criteria parser、GUI preview parser parity、criteria kind 不意外泄漏、revision 清理 closure、completed pending closure 可见、pending closure 补证据 stale、长 timeline 截断下 final audit 仍使用 DB 最新 `goal_linked` marker、accepted v1 closure 后端 guard、strict evidence reopen、durable follow-up 追加/去重/sealed guard、workflow criteria 绑定、invalid criteria fail-closed、既有 workflow/validation/diff/worktree/review evidence。
- `cargo test -p ha-core loop_control --locked`：13 个 loop 相关匹配测试通过，覆盖 criteria 绑定从 Loop schedule 传到 admission 与派生 WorkflowRun。
- `pnpm vitest run src/components/chat/workspace/goalCriteriaDraft.test.ts src/components/chat/input/ChatInput.test.tsx src/components/chat/workspace/WorkspacePanel.test.tsx`：86 个 GUI 相关测试通过，覆盖输入框目标模式发送、active Goal 下追加 follow-up、active Goal strip 进度、Goal 创建、Goal detail closure packet 复制、criteria 草稿预览、criteria 级 Workflow/Loop/Evidence 计数、audit stale 禁用 accepted-v1、worktree/domain evidence 展示和既有 Workflow/Loop workspace 交互。
- `cargo check -p ha-server --locked`：HTTP owner endpoint 编译通过。
- `cargo check -p hope-agent --locked`：Tauri owner command 编译通过。
- `pnpm typecheck`：前端 Goal v2 类型和 GUI 接线通过。

仍未声称完成的严格证据：

- 没有跑全量 `pnpm test` / `pnpm lint` / clippy；按项目开发期约定只做 targeted 验证。
- 没有做真实浏览器截图或手动视觉 smoke；当前为 source-level + jsdom GUI 验证。

退出标准：

- Goal v2 关键路径不依赖 slash command。
- 用户修改目标后模型下一轮能感知。
- final audit 能生成可复核 closure packet。
- 后续增强不会继续拖住当前 Goal。
- active Goal 输入框支持更新、替代、追加必须项、追加可选项、追加后续项；Goal detail 支持复制 closure packet 和追加 follow-up。

## 9. 与 Loop v2 的关系

Goal v2 应先于 Loop v2 完成。

原因：

- Loop 是持续推进器，必须知道推进哪个 Goal / criteria。
- 没有清晰 stop condition 的 Loop 容易空转。
- Goal v2 的 closure packet 和 follow-up pool 可以告诉 Loop 什么时候停、什么时候降频、什么时候请求用户。

Loop v2 不应重新定义 Goal 完成标准；它只读取 Goal v2 的 criteria、budget、blocker 和 closure state。

## 10. 后续增强池

这些不阻塞当前 Goal v2 完整闭环：

- 多 Goal 并行。
- 跨 session / 跨 project Goal。
- Goal 模板市场。
- 自动把 follow-up pool 转 GitHub issue / Linear task / Calendar reminder。
- LLM auditor 全量接入 final audit。
- 复杂 OKR / KPI 层级树。
