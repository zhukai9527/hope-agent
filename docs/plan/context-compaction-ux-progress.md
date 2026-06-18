# 上下文压缩用户提示与进度展示方案

> 状态：实施方案 v3（Phase 1/2 已落地，Phase 3 待做）
> 关联方案：[`docs/plan/context-compact-state-transfer.md`](context-compact-state-transfer.md)、[`docs/plan/mid-loop-context-compaction.md`](mid-loop-context-compaction.md)
> 目标架构文档：[`docs/architecture/context-compact.md`](../architecture/context-compact.md)、[`docs/architecture/chat-engine.md`](../architecture/chat-engine.md)
> 目标：把上下文压缩从“工程调试事件”改造成用户能理解的后台压缩过程，同时保留 manifest / tier 等诊断信息给开发者。
> v2 修订重点：完成态以 final `context_compacted` 为唯一真相源；progress 不 carry `phase/kind` 到 final；Tier 4 通过 `kind="emergency"` 表达，不新增不在类型内的 phase；文件恢复 / runtime ledger 阶段改为条件展示。

## 背景

上下文压缩是 Hope Agent 能长期运行的基础能力。当前本分支已经有：

- Tier 0/1/2/3/4 分层压缩。
- Tier 3 摘要开始事件：`description = "summarizing"`。
- Tier 4 紧急压缩开始事件：`description = "emergency_compacting"`。
- 最终 `context_compacted` event，携带 `tier_applied`、`messages_affected`、`tokens_before`、`tokens_after`、`manifest`。
- GUI 侧临时 banner：start marker / progress event 显示 spinner，最终 `context_compacted` 替换它。
- GUI / IM 默认文案已不展示 tier；tier / manifest 只作为诊断字段保留。
- IM 侧保持 suppress start marker，只显示最终友好通知，避免刷屏。

相关文件：

- `crates/ha-core/src/agent/context.rs`：Tier 3 summarizing start marker 与 final `context_compacted` event。
- `crates/ha-core/src/chat_engine/engine.rs`：Tier 4 emergency start marker 与 final event。
- `crates/ha-core/src/chat_engine/persister.rs`：只持久化 Tier ≥ 2 的最终事件，跳过 start marker。
- `crates/ha-core/src/chat_engine/im_system_message.rs`：IM 侧系统通知文案。
- `src/components/chat/hooks/useStreamEventHandler.ts`：GUI 侧 start marker 合并 / 替换逻辑。
- `src/components/chat/ContextCompactedBanner.tsx`：GUI 侧上下文压缩 banner。
- `src/types/chat.ts`：`ContextCompactedEvent` / `ContextCompactionProgressEvent` 类型。
- `src/i18n/locales/*.json`：相关 UI 文案。

历史体验的问题是：用户能看到压缩发生了，但看到的是 “Tier 2 / Tier 3” 这类内部实现词，且摘要过程只有“开始 / 结束”。本方案 v2 已把“去工程化文案”作为 Phase 1 收敛，后续重点是把阶段式 progress 的语义边界定清楚。

## 当前问题

### 1. 用户可见文案曾暴露实现细节

早期 GUI banner 会展示：

```text
Context compacted · Tier 3 · 12 messages
```

早期 IM 通知会展示：

```text
Context compacted (tier 3, 12 msgs)
```

问题：

- “Tier 1 / Tier 2 / Tier 3” 是工程分层，不是用户心智。
- 用户不关心用了第几层策略，只关心“是不是还能继续当前任务”。
- 这些词看起来像系统报错或调试日志，降低信任感。

### 2. 摘要压缩仍需要更细的阶段式过程展示

当前 GUI 已经有：

```text
开始：description = "summarizing"，显示 spinner
结束：最终 context_compacted event，替换 start marker
```

但完整阶段仍未完全落地：

```text
准备压缩历史
生成接续摘要
恢复最近编辑文件
保留后台任务状态
完成 / 失败
```

如果只显示一个模糊 spinner，长摘要调用期间仍会显得像卡住。mid-loop 摘要加入后，当前回复可能在工具循环中停顿一段时间，更需要解释“系统正在做什么”。

### 3. GUI 与 IM 行为不一致

GUI：

- 显示 start marker。
- final event 替换 start marker。

IM：

- `im_system_message.rs` suppress `summarizing` / `emergency_compacting` start marker。
- 只显示 final event。

这个差异不是一定错误，但需要明确产品策略：IM 是否也应该展示“正在生成摘要”，以及如何避免刷屏。

### 4. manifest 是诊断信息，不应直接当用户文案

`manifest` 很适合开发者排障：

- tier
- token before / after
- protected boundary
- recovered files
- warnings

但普通聊天界面不应默认展示这些字段。它们应该进入 tooltip / expandable details / debug 面板。

## 设计目标

1. 用户可见文案不再直接出现 `Tier 1/2/3/4`。
2. 用户能理解压缩是在“压缩较早上下文，让当前任务继续”。
3. Tier 3 / Tier 4 有明确的进行中状态。
4. 支持阶段式 progress，但不伪造百分比。
5. GUI 与 IM 的策略清晰：GUI 展示过程，IM 默认简洁。
6. 技术细节保留给 manifest / debug detail，不丢可观测性。
7. 方案兼容后续 mid-loop compaction checkpoint。

## 非目标

- 不改变压缩算法本身。
- 不改变 manifest 结构作为诊断载体的价值。
- 不做 token-level 真进度条；摘要 LLM 调用本身没有可靠百分比。
- 不为 Tier 0 / Tier 1 这类轻量压缩制造用户通知。
- 不把每个 progress 阶段都持久化成聊天历史消息。

## 用户心智模型

把上下文压缩对用户解释为：

```text
Hope Agent 正在压缩较早的对话，保留当前任务需要的信息，让对话可以继续变长。
```

用户可见状态应是“动作 + 目的”，不是“实现层级”。

建议用语：

| 内部事件 | 用户文案 |
| --- | --- |
| Tier 2 | 已压缩较早上下文 |
| Tier 3 summarizing | 正在生成会话摘要 |
| Tier 3 complete | 已生成会话摘要 |
| Tier 4 emergency | 上下文太长，正在快速释放空间 |
| Tier 4 complete | 已压缩上下文，继续尝试 |
| Tier 3 failed | 暂时无法生成摘要，将继续尝试当前对话 |

## 事件设计

### 保留现有 `context_compacted`

现有事件继续存在，向后兼容：

```json
{
  "type": "context_compacted",
  "data": {
    "tier_applied": 3,
    "description": "summarizing",
    "messages_to_summarize": 12
  }
}
```

最终事件继续：

```json
{
  "type": "context_compacted",
  "data": {
    "tier_applied": 3,
    "description": "summarization_needed",
    "messages_affected": 12,
    "tokens_before": 95000,
    "tokens_after": 42000,
    "manifest": {}
  }
}
```

但 UI 不再直接使用 `tier_applied` 生成用户文案。

### 完善 live-only `context_compaction_progress`

使用轻量 progress event：

```json
{
  "type": "context_compaction_progress",
  "data": {
    "phase": "summarizing",
    "kind": "summary",
    "messages_to_summarize": 12
  }
}
```

建议字段：

```ts
type ContextCompactionPhase =
  | "preparing"
  | "summarizing"
  | "restoring_files"
  | "preserving_runtime_state"
  | "finalizing"
  | "failed"

interface ContextCompactionProgressEvent {
  phase: ContextCompactionPhase
  kind?: "cleanup" | "summary" | "emergency"
  messages_to_summarize?: number
  files_recovered?: number
  warning_count?: number
}
```

注意：

- `message` 这类字段可选，最好不传最终用户文案，避免后端硬编码语言。
- 前端根据 `phase/kind` 做 i18n。
- start marker 可以复用 `context_compacted`，也可以逐步迁移到 progress event。
- **完成态不使用 progress 作为真相源**：final `context_compacted` 是唯一完成事件。若为兼容旧实现收到 `phase = "done"` 的 progress，前端应忽略或仅作为 raw stream 诊断，不渲染第二个完成状态。
- `phase` 只表达通用处理阶段；Tier 4 不新增 `emergency_compacting` phase，而是用 `kind = "emergency"` + `phase = "preparing" | "finalizing" | "failed"` 表达。

## 阶段式进度

不用百分比，使用阶段：

```text
preparing
→ summarizing
→ preserving_runtime_state?   # 仅实际有 live job/subagent/ledger 状态时
→ restoring_files?            # 仅实际恢复文件内容时
→ finalizing
→ final context_compacted
```

对于 Tier 2：

```text
通常只发 final context_compacted；不为轻量同步裁剪制造额外过程提示。
如果未来某个 Tier 2 路径变慢，再考虑 preparing → finalizing → final context_compacted。
```

对于 Tier 4：

```text
preparing
→ finalizing
→ final context_compacted
```

更具体地说：

| phase | 触发位置 | 展示文案 |
| --- | --- | --- |
| `preparing` | 选定 summarizable prefix 后 | 正在压缩较早上下文 |
| `summarizing` | 调用摘要模型前 | 正在生成会话摘要 |
| `preserving_runtime_state` | 确认有 live background job / subagent / ledger 状态需要注入时 | 正在保留后台任务状态 |
| `restoring_files` | 确认有文件内容将被 recovery 注入时 | 正在恢复最近编辑文件 |
| `finalizing` | apply summary / insert messages 后 | 正在完成上下文压缩 |
| `failed` | summary error / timeout | 摘要未完成，将继续当前对话 |

`done` 不作为 progress phase 使用；完成文案由 final `context_compacted` 渲染。

## GUI 设计

### Banner 形态

文件：`src/components/chat/ContextCompactedBanner.tsx`

当前是小 chip：

```text
[spinner] Compacting context · Tier 3 · 12 messages
```

建议改成：

```text
[spinner] 正在生成会话摘要 · 压缩 12 条较早消息
```

完成：

```text
[archive] 上下文已压缩 · 保留最近对话，释放约 53k tokens
```

紧急：

```text
[spinner] 上下文太长，正在快速释放空间
[archive] 已压缩上下文，继续尝试
```

### 不直接展示 tier

`tier_applied` 不再进入默认 subtitle。

默认 subtitle 优先展示：

- `messages_to_summarize` / `messages_affected`
- token saved：`tokens_before - tokens_after`
- recovered files count

示例：

```text
已生成会话摘要 · 压缩 12 条较早消息
上下文已压缩 · 释放约 18k tokens
```

### 技术详情入口

如果需要保留可见诊断信息，可以加 tooltip 或展开详情：

```text
技术详情
Tier 3 · 95k → 42k tokens · protectedStartIndex=18 · recoveredFiles=2
```

默认不展开。

## IM 设计

文件：`crates/ha-core/src/chat_engine/im_system_message.rs`

IM 更容易刷屏，建议：

### 初版

保持 suppress start marker，只改最终文案：

```text
📚 _已压缩较早上下文，当前任务可以继续。_
📚 _已生成会话摘要，压缩了 12 条较早消息。_
📚 _上下文太长，已快速释放空间并继续尝试。_
```

不要显示：

```text
Context compacted (tier 3, 12 msgs)
```

### 后续可选

如果 mid-loop 摘要耗时较长，IM 可以显示一条“正在生成摘要…”并用 final 替换。IM 消息替换能力不一定和 GUI 一样自然，因此默认不做，避免一来一回两条系统消息。

## 持久化策略

文件：`crates/ha-core/src/chat_engine/persister.rs`

现状：

- Tier 0/1 不持久化。
- start marker 不持久化。
- Tier ≥ 2 final event 持久化。

建议保持：

- progress event 不持久化。
- start marker 不持久化。
- final event 持久化。

原因：

- 历史回放时只需要知道“压缩已发生”，不需要看到过去的进行中状态。
- 避免 reload 后显示“正在生成摘要”这种过期状态。

## 前端事件处理

文件：

- `src/components/chat/hooks/useStreamEventHandler.ts`
- `src/components/chat/hooks/useStreamEventHandler.test.ts`

建议：

1. 继续保留 start marker 替换 final event 的机制。
2. 新增 `context_compaction_progress` 后，复用同一条 event message，而不是追加多条。
3. final `context_compacted` 替换 live notice 时，只 carry 不会改变最终状态语义的事实字段：
   - `messages_to_summarize`
   - `progress_started_at` 可选
4. 不 carry `phase` / `kind` 到 final event。final event 如果没有 `phase`，就按完成态渲染；否则会把 `summarizing` 之类的进行中状态误带到完成 banner。
5. Tier 0/1 继续 suppress。

## 类型与 i18n

文件：

- `src/types/chat.ts`
- `src/i18n/locales/en.json`
- `src/i18n/locales/zh.json`
- 其余 locale 文件

新增类型：

```ts
export interface ContextCompactionProgressEvent {
  phase?: "preparing" | "summarizing" | "restoring_files" | "preserving_runtime_state" | "finalizing" | "failed"
  kind?: "cleanup" | "summary" | "emergency"
  messages_to_summarize?: number
  files_recovered?: number
}
```

建议 i18n key：

```json
{
  "chat.contextCompaction.preparing": "正在压缩较早上下文",
  "chat.contextCompaction.summarizing": "正在生成会话摘要…",
  "chat.contextCompaction.restoringFiles": "正在恢复最近编辑的文件…",
  "chat.contextCompaction.preserveRuntime": "正在保留后台任务状态…",
  "chat.contextCompaction.finalizing": "正在完成上下文压缩",
  "chat.contextCompaction.summaryDone": "已生成会话摘要",
  "chat.contextCompaction.emergency": "上下文太长，正在快速释放空间…",
  "chat.contextCompaction.emergencyDone": "已压缩上下文，继续尝试",
  "chat.contextCompaction.messages": "压缩 {{count}} 条较早消息",
  "chat.contextCompaction.savedTokens": "释放约 {{count}} tokens",
  "chat.contextCompaction.details": "技术详情"
}
```

旧 key `chat.contextCompactedTier` 可以保留给 debug detail，不再在默认 banner 使用。

## 与 mid-loop 摘要的关系

mid-loop 摘要会让压缩发生在一次回复内部。此时用户可能看到 assistant 停顿，尤其是长工具任务里：

```text
工具返回
→ 正在生成会话摘要
→ 继续下一轮模型请求
```

因此 mid-loop 方案落地前，至少应完成：

- 不显示 tier。
- 有“正在生成会话摘要”的 start marker。
- final event 能替换 start marker。

阶段式 progress 可以与 mid-loop 同 PR 或紧接着做。

## 实施步骤

### Phase 1：文案去工程化（本分支已落地）

文件：

- `src/components/chat/ContextCompactedBanner.tsx`
- `crates/ha-core/src/chat_engine/im_system_message.rs`
- `src/i18n/locales/*.json`
- `src/components/chat/hooks/useStreamEventHandler.test.ts`
- `crates/ha-core/src/chat_engine/im_system_message.rs` 测试

内容：

- 默认 UI 不展示 `tier_applied`。
- GUI 文案改为“正在生成会话摘要 / 上下文已压缩 / 快速释放空间”。
- IM 文案改为用户友好版本。
- Tier 0/1 继续 suppress。

### Phase 2：阶段式 progress event（已落地）

文件：

- `crates/ha-core/src/agent/context.rs`
- `crates/ha-core/src/chat_engine/engine.rs`
- `src/components/chat/hooks/useStreamEventHandler.ts`
- `src/components/chat/ContextCompactedBanner.tsx`
- `src/types/chat.ts`

内容：

- 新增 `context_compaction_progress` live-only event。
- Tier 3 在 preparing / summarizing / finalizing 发阶段事件；`preserving_runtime_state` / `restoring_files` 只有实际有 ledger / recovery 注入时才发。
- Tier 4 用 `kind = "emergency"`，在 preparing / finalizing 发阶段事件，不新增 `emergency_compacting` phase。
- 前端同一条 banner 原地更新。
- final `context_compacted` 是完成态唯一真相源；progress `done` 不作为新事件发出，旧事件前端忽略。

### Phase 3：debug details

文件：

- `src/components/chat/ContextCompactedBanner.tsx`
- `src/types/chat.ts`

内容：

- 默认只展示用户文案。
- tooltip / expandable detail 展示 tier、tokens、manifest warnings。
- 仅在用户展开时看技术字段。

## 测试计划

### 前端单元测试

文件：`src/components/chat/hooks/useStreamEventHandler.test.ts`

覆盖：

1. `summarizing` start marker 显示为进行中 banner。
2. final event 替换 start marker。
3. Tier 0/1 继续 suppress。
4. progress event 原地更新，不追加多条 event message。
5. banner 默认不包含 `Tier` 文案。
6. final event 替换 progress 时不 carry `phase` / `kind`，避免完成 banner 继续转圈。
7. `phase = "done"` 的 progress 兼容输入被忽略，不渲染第二个完成态。
8. Tier 4 progress 使用 `kind = "emergency"`，不依赖不在类型内的 `emergency_compacting` phase。

### Rust 单元测试

文件：`crates/ha-core/src/chat_engine/im_system_message.rs`

覆盖：

1. Tier 3 final 文案不出现 `tier`。
2. Tier 4 final 文案是“快速释放空间 / 继续尝试”语义。
3. Tier 0/1 仍返回 `None`。
4. start marker 初版仍 suppress，或如果选择展示，确保不会重复 final。

### 手动验证

1. 调低 `summarizationThreshold`，触发 Tier 3。
2. GUI 看到“正在生成会话摘要”，最终替换为“已生成会话摘要”。
3. 刷新会话后只看到最终完成消息，不看到过期 spinner。
4. IM 渠道只收到友好 final 通知。

## 验收标准

1. 普通聊天 UI 默认不出现 `Tier 1/2/3/4`。
2. 摘要开始时 GUI 有明确进行中状态。
3. 摘要结束后进行中状态被最终状态替换。
4. 长摘要调用期间用户能理解系统没有卡死。
5. manifest / tier 仍可用于日志和 debug detail。
6. IM 通知不刷屏，且不暴露工程词。
7. 与 mid-loop 摘要兼容。
8. 完成态只有 final `context_compacted` 一个真相源；progress 不制造第二个完成事件。
9. `preserving_runtime_state` / `restoring_files` 只在实际有 ledger / recovery 内容时展示。

## 开放问题

1. IM 是否需要显示开始态，还是只显示最终态。本文建议初版只显示最终态。
2. debug detail 是 tooltip、popover，还是折叠展开行。
3. token saved 是否适合面向普通用户展示，还是只放进 debug detail。本文倾向先不默认展示，除非用户打开技术详情。
