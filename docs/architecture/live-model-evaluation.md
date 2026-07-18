# 真实模型与复杂任务评测

> 状态：Hope Core 基础设施 v1 已实现；真实模型结果初始为 advisory。
> 确定性评测：[`capability-eval.md`](capability-eval.md)

## 1. 已实现边界

Hope Agent 有两条物理分离的能力评测轨道：

| 轨道 | 命令域 | Evidence | 是否调用模型 | 发布地位 |
| --- | --- | --- | --- | --- |
| 确定性专项评测 | `hope-agent-eval validate/plan/run/...` | `eval-evidence.v1` | 禁止 | 独立发布证据 |
| 真实模型 Campaign | `hope-agent-eval model ...` | `eval-model-campaign.v1` | 显式确认后会调用 | 初始 advisory，未来只能额外加门，不能替代确定性证据 |

真实模型轨道已经实现：

- 独立 scenario、suite、policy、plan、trial、shard、evidence 和 waiver 类型及 JSON Schema；
- 受限 YAML、canonical JSON、SHA-256 component digest、稳定 trial seed/ID、append-only version lock；
- `validate`、`plan`、稳定 hash 分片、隔离 trial 子进程、三态聚合、exact-SHA evidence 验证；
- 仅基础设施/模拟器错误自动重试一次，业务失败、策略失败和预算耗尽不重试；
- 混合 `k=3/k=5` 的 `any_pass@k` / `all_pass@k` 分组统计、Hard Success 95% Wilson 区间、成功样本 p50/p95；
- 单 trial 与整 campaign 两级时间、模型调用、Token、费用、工具、Agent 和并发预算；
- Hope Server 黑盒 adapter、确定性终态/文件/Git/trace verifier、28 个 Hope Core 场景资产；
- `EvalRunContext` 对模型、工具、Goal、Loop、Workflow、Checkpoint、异步任务、Subagent 和 Team 的贯穿归因；
- 结构化、无正文的因果事件；模型事件记录 Provider/模型/Token/TTFT，工具事件只记录名称与参数/结果摘要；
- Goal/Workflow/子 Agent 检查点后的真实进程重启、用户事件两阶段前置状态检查、control/faulted 与计算量匹配 solo/team 对照臂；
- 运行期硬预算；重启只继承剩余额度，不能通过重启或 infra retry 重置成本；
- 零费用 fake Provider 黑盒 smoke，真实启动 Hope Server 并覆盖模型、工具、Goal、Loop、Workflow、Async 和 Team 归因；
- 专用 `Live Model Campaign` workflow 和 Release 的独立 model-evidence preflight。

外部 BFCL/AppWorld/Gaia2/Terminal-Bench/τ³/TeamBench/CooperBench/MCPMark/OSWorld 的 adapter 名已注册，未安装 Harness 时会明确产生 `benchmark_defect`，绝不伪造通过。带结构化故障但尚无对应受审故障控制器的场景同样 fail closed。注册不等于实现完成；具体接入顺序见 Roadmap。

当前自动 policy 已展开为跨能力 pilot：nightly 8 case / 20 trial，weekly 25 case / 268 trial，release 22 case / 250 trial，monthly 28 case / 74 trial。它覆盖 Goal/Loop、Workflow、Async Jobs、Subagent/Team 和混合 E2E，并为带故障场景生成 clean/control 与 chaos/faulted 对照，为多 Agent 场景生成 compute-matched solo/team 对照。这些数字描述的是不可变计划，不等于已经获得真实 Provider 基线；第一次受保护 Runner 运行后仍需检查任务可解性、模型噪声和 grader 假阳性，再决定 quarantine 或阈值。

## 2. 入口

```bash
# 只校验资产，不调用模型
cargo run -p ha-eval --locked -- model validate

# 固化精确 SHA 的不可变计划，不调用模型
cargo run -p ha-eval --locked -- model plan \
  --tier nightly --ref <40位commit-sha> --output model-plan.json

# 会调用所选真实模型 API；本地必须显式确认费用
cargo run -p ha-eval --locked -- model run \
  --plan model-plan.json \
  --suite hope-core-orchestration \
  --shard 1/4 \
  --output shard.json \
  --confirm-model-costs

cargo run -p ha-eval --locked -- model aggregate \
  --plan model-plan.json --inputs ./shards \
  --output eval-model-campaign.v1.json \
  --summary model-summary.md
```

`model run` 需要一个单独启动、配置真实 Provider 的 Hope Server：

```text
HA_MODEL_EVAL_MODE=1
HA_MODEL_EVAL_SERVER_URL=http://127.0.0.1:<port>
HA_MODEL_EVAL_SERVER_TOKEN=<专用server-token>
```

隔离 `HA_DATA_DIR/config.json` 只保存无凭据 Provider/模型配置，`apiKey` 必须为空且不得包含 `authProfiles`。模型 Key 由受保护的 `HA_MODEL_EVAL_PROVIDER_SECRETS_B64` 临时注入 Hope Server：内容是 `providerId -> apiKey` 的 base64 JSON 对象，首次加载配置后立即从 Server 环境移除，只保留在进程内存中；评测模式同时禁止配置写回，避免内存中的 Key 落盘。Runner 为每个 trial 创建临时任务目录，并从 Harness 子进程环境删除 Provider Key/token/Cookie 等常见凭据变量。Runner 不直接调用 Provider。

Trial evidence 额外记录这个无凭据 `config.json` 的 SHA-256；聚合 evidence 记录 Runner OS/架构。release enforce 要求所有有效 trial 使用同一个运行配置摘要，并同时具备不可变模型与价格快照，防止配置漂移被误认为同一基线。

App 已有 Coding / Domain 的真实模型 Campaign 入口继续保留，适合本地显式评测；它和 CLI 本地产物都只能标记为 local source，不能晋升为 release evidence。

## 3. 资产和不可变计划

```text
evals/live/
  schema/          # 两轨不兼容的 JSON Schema
  policy/          # nightly / weekly / release / monthly
  suites/          # 注册 adapter、模型角色、分片和 case
  scenarios/       # 版本化任务、公开 fixture、隐藏 truth、verifier
  version-lock.json
```

Manifest 只能引用注册枚举和相对资产路径，不能携带 shell 命令。路径 canonicalize 后必须仍位于对应 scenario 或 `evals/live` 内，symlink/`..` escape 会失败。Scenario digest 同时固定 manifest、公开资产、隐藏 truth、Prompt、工具 schema、verifier 和环境声明。

`model plan` 会重新读取当前 policy/suite/scenario，展开模型角色、重复次数和 trial seed。之后每次 `run`、`aggregate`、`verify-evidence` 都重建计划并做逐字段比较；同版本资产发生变化必须提升版本并追加 lock。

## 4. Hope 黑盒执行和评分

`hope_core_scenario` 只通过生产 HTTP 入口 `POST /api/chat` 驱动 Hope：

1. 把公开资产复制到 per-trial 临时 workspace，隐藏 truth 不进入被测目录；
2. 传入精确 `providerId::modelId`、working directory、可选初始 Goal 与不可变 `EvalRunContext`；
3. 等真实 Agent loop、工具和控制面完成；
4. 读取 owner API、文件、Git 状态以及只读 eval telemetry；
5. 由注册 verifier 判断终态，不以最终回答中的“我完成了”作为成功；
6. 关闭根 trace 后生成 trial result。

v1 注册 verifier 包括：

- `hope_state_subset`：只允许审核过的 loopback owner API 路径；
- `file_exists`、`file_contains_all`、`file_json_subset`；
- `git_changed_paths`；
- `signal_observed`、`trace_closed`；
- `response_non_empty`、`response_contains_all`、`response_json_subset`。

blocking milestone/invariant 任一失败时，trial 不得为 `passed`。`false completion`、权限/作用域、安全副作用等护栏不得被平均分或 waiver 抵消。

## 5. 归因与隐私

`EvalRunContext` 仅在 `HA_MODEL_EVAL_MODE=1` 的隔离 Server 中接受。上下文至少包含 campaign/case/trial/trace/root span/model role/seed；它随 Session 传播到 Subagent 和后台任务。

Registry 有明确上限：最多保留 256 个 trial、每 trial 4096 个事件。事件只保存稳定 ID、状态、序号、时长和受限标量属性，不保存 Prompt、模型正文、工具参数、工具输出或数据库记录。工具参数与结果只写 SHA-256；模型事件只写 Provider/模型、调用类型、Token、TTFT、成功状态和错误类别。主回复后由产品自动触发的 session title、Memory extraction 等模型调用会继续持有 trial 身份，Token/费用纳入总量，同时以 `model_automation.run` 与任务型 Async Job 分开；预算不足时不允许产生未归因调用。模型 usage metadata 只增加 `metadata.eval` 身份；正常 App/Server/ACP 请求没有 eval context 时不会注册 trial。

Evidence 写出前会扫描常见 Key/token/Cookie、Authorization、私钥片段和个人绝对路径。真实模型评测**会**调用 Provider API、消耗 Token 并按价格快照计费；安全限制的含义是：

- 使用组织单独创建的评测账号/API Key，设置最小权限、速率和费用上限，不复用开发者个人生产账号；
- 任务输入使用仓库内合成 fixture，或已经获得授权并完成脱敏的数据，不复制真实用户会话、文件和隐私；
- Runner 防火墙只放行模型 Provider 与 scenario 明确批准的 fixture service，不给工具任意访问公网的能力。

因此，“禁止无限制外网”不等于“禁止联网”，而是把联网范围缩到完成该 trial 所必需的目标。App 现有 Coding/Domain 真实模型入口不受影响，用户显式运行时仍按所选 Provider 正常调用 API。

## 6. CI 和发布

`.github/workflows/model-campaign.yml` 只有 schedule 与 `workflow_dispatch`，没有 PR/push 触发，也不是 branch protection required check。

- `prepare` 使用 hosted runner，只做构建、资产验证和不可变计划；
- `run` 使用受保护 `model-eval` environment 与 `[self-hosted, linux, x64, model-eval]` disposable runner；
- Runner fleet 必须在外部防火墙实施 provider-only egress，`HA_MODEL_EVAL_NETWORK_ENFORCED=1` 只是由二进制强制检查的部署证明，不是防火墙本身；
- 每个分片启动独立 Hope Server/Data Dir；无凭据 Provider 配置由 `MODEL_EVAL_CONFIG_B64` 注入，Provider Key 映射由 `MODEL_EVAL_PROVIDER_SECRETS_B64` 注入，server token 由 `MODEL_EVAL_SERVER_TOKEN` 注入；
- 运行完成后停止 Server 并销毁隔离数据；weekly 保留 30 天、release 保留 90 天；
- release waiver 只接受手动触发、受保护 `release-model-eval-waiver` environment 审批、精确 SHA/tag/理由/suite，一次有效。

Release workflow 会查找同一 SHA 的 `release-model-evidence-<sha>`，重新验证版本、SHA、dirty、source、policy、suite/scenario/model/runtime-config digest、完整 trial 集和 attribution。当前 `mode=advisory` 时，trial 失败、infra 比例、护栏和预算问题进入醒目摘要但不阻断发布；schema、artifact hash、secret scan、来源、clean exact-SHA、计划/digest 和 waiver 完整性仍始终 fail closed。切换 `enforce` 只能通过 policy PR，届时 readiness、护栏、预算和失败 suite 才成为发布门禁。

## 7. 维护契约

- 普通 PR、pre-push、默认 Cargo test 不运行真实模型 Campaign，也不需要 Provider Key。
- 修改 schema/policy/suite/scenario/verifier/Prompt/tool schema 时必须提升对应版本并更新 `evals/live/version-lock.json`。
- 新 adapter/verifier/fault 只能在 Rust/Python Harness 的注册代码中实现，Manifest 禁止命令字符串。
- 新的异步或多 Agent 执行边界必须传播 `EvalRunContext`，终态必须关闭对应 guard；release trial attribution 不是 `complete` 时证据无效。
- 工具调用少、Token 少、速度快都不能单独覆盖任务失败；效率只在成功样本上比较，并同时展示全量失败率。
- 外部 benchmark 版本、镜像 digest、许可证和 grader 必须先审计，再加入 allowlist 与 version lock。
