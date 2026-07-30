import type {
  ProjectMeta,
  ProjectWorkflowDiscovery,
  ProjectWorkflowFixedArtifact,
  ProjectWorkflowGateContract,
  ProjectWorkflowPreview,
  ProjectWorkflowTemplateSummary,
  ProjectWorkflowVerificationCommand,
} from "@/types/project"
import type { TaskProgressSnapshot } from "@/components/chat/tasks/taskProgress"

export type TaskDeliverySource = "project-workflow" | "built-in" | "none"
export type TaskDeliveryItemStatus = "pending" | "active" | "blocked" | "completed" | "skipped" | "unknown"
export type TaskDeliveryArtifactStatus = "expected" | "missing" | "draft" | "valid" | "invalid" | "skipped" | "unknown"
export type TaskDeliveryVerificationStatus = "not_run" | "running" | "passed" | "failed" | "skipped" | "unknown"
export type TaskDeliveryEvidenceStatus = "passed" | "failed" | "warning" | "unknown"
export type TaskDeliveryActionKind = "start" | "continue" | "verify" | "close"
export type TaskDeliveryPhaseActionKind = "continue" | "evidence" | "blockers"
export type TaskDeliveryVerificationActionKind = "run" | "skip" | "explain"

export interface TaskDeliveryEvidenceCheck {
  id: string
  label: string
  status: TaskDeliveryEvidenceStatus
  detail?: string | null
}

export interface TaskDeliveryGateContract {
  id: string
  label: string
  phaseId?: string | null
  requiredArtifacts?: string[]
  requiredTerms?: string[]
  source?: string | null
}

export interface TaskDeliveryActionState {
  id: TaskDeliveryActionKind
  label: string
  description: string
  enabled: boolean
  disabledReason?: string | null
}

export function buildTaskDeliveryActionPrompt(
  state: Pick<
    TaskDeliveryState,
    "projectName" | "taskDir" | "taskStatus" | "currentPhaseId" | "currentStepId" | "templateId" | "templateName"
  >,
  action: TaskDeliveryActionKind,
): string {
  const projectPart = state.projectName ? `项目「${state.projectName}」` : "当前项目"
  const taskContext = [
    state.taskDir ? `任务目录：${state.taskDir}` : null,
    state.templateName || state.templateId ? `流程：${state.templateName ?? state.templateId}` : null,
    state.currentPhaseId ? `当前阶段：${state.currentPhaseId}` : null,
    state.currentStepId ? `当前步骤：${state.currentStepId}` : null,
    state.taskStatus ? `任务状态：${state.taskStatus}` : null,
  ].filter(Boolean)

  const suffix = taskContext.length > 0 ? `\n\n上下文：\n${taskContext.map((item) => `- ${item}`).join("\n")}` : ""

  switch (action) {
    case "start":
      return `${projectPart}：启动项目 Hope workflow。请先确认 TAPD ID、任务类型、标题、目标分支、涉及 package/plugin 和验证命令；不要直接执行 Git/TAPD 外部交付动作。${suffix}`
    case "continue":
      return `${projectPart}：继续 ${state.taskDir ?? "当前任务目录"} 的项目 Hope workflow。请读取 progress.json 和任务资产，按当前阶段推进；遇到需求/设计确认门禁时暂停询问用户，不要直接执行 Git/TAPD 外部交付动作。${suffix}`
    case "verify":
      return `${projectPart}：对 ${state.taskDir ?? "当前任务目录"} 跑项目 Hope workflow 验证阶段。请先根据 docs/project-matrix.md 和 test_cases.md 确认可用验证命令，再由对话流程承接执行与结果记录；不要把 Playwright 当作 tsc/build/lint 的替代。${suffix}`
    case "close":
      return `${projectPart}：对 ${state.taskDir ?? "当前任务目录"} 执行项目 Hope workflow 收口检查。请检查标准任务资产、progress.*、interaction_log.md 和剩余风险；Git 提交/push/MR 与 TAPD 回写必须在明确确认后才执行。${suffix}`
  }
}

export function buildTaskDeliveryArtifactRepairPrompt(
  state: Pick<TaskDeliveryState, "projectName" | "taskDir" | "currentPhaseId" | "templateId" | "templateName">,
  artifact: Pick<TaskDeliveryArtifactState, "name" | "path" | "status" | "evidenceChecks">,
): string {
  const projectPart = state.projectName ? `项目「${state.projectName}」` : "当前项目"
  const artifactPath = artifact.path ?? (state.taskDir ? `${state.taskDir}/${artifact.name}` : artifact.name)
  const failedChecks = artifact.evidenceChecks?.filter((check) => check.status !== "passed") ?? []
  const checkText = failedChecks.length
    ? `\n\n需要补齐的证据：\n${failedChecks.map((check) => `- ${check.label}${check.detail ? `：${check.detail}` : ""}`).join("\n")}`
    : ""
  const workflowText = state.templateName || state.templateId ? `\n- 流程：${state.templateName ?? state.templateId}` : ""
  const phaseText = state.currentPhaseId ? `\n- 当前阶段：${state.currentPhaseId}` : ""

  return `${projectPart}：请补齐或修复任务交付产物 ${artifactPath}。\n\n要求：\n- 先读取任务目录下的 progress.json 和相关已有产物。\n- 如果涉及 design.md，必须引用 docs/project-matrix.md 并定位到具体 workspace/package/plugin/配置/文档范围。\n- 如果涉及 test_cases.md，必须区分 tsc、build、lint/format、Playwright 的执行结果或未执行原因。\n- 如果涉及 interaction_log.md，必须保留表格格式并记录用户确认依据。\n- 只修改与该产物直接相关的内容，不执行 Git/TAPD 外部交付动作。\n\n上下文：\n- 任务目录：${state.taskDir ?? "未绑定"}\n- 产物状态：${artifact.status}${workflowText}${phaseText}${checkText}`
}

export function buildTaskDeliveryPhaseActionPrompt(
  state: Pick<TaskDeliveryState, "projectName" | "taskDir" | "currentPhaseId" | "currentStepId" | "templateId" | "templateName">,
  phase: Pick<TaskDeliveryPhaseState, "id" | "name" | "status" | "gateStatus" | "gateChecks" | "requiredInteractionCount">,
  action: TaskDeliveryPhaseActionKind,
): string {
  const projectPart = state.projectName ? `项目「${state.projectName}」` : "当前项目"
  const failedChecks = phase.gateChecks?.filter((check) => check.status !== "passed") ?? []
  const checkText = failedChecks.length
    ? `\n\n未满足的阶段证据/门禁：\n${failedChecks.map((check) => `- ${check.label}${check.detail ? `：${check.detail}` : ""}`).join("\n")}`
    : ""
  const context = `\n\n上下文：\n- 任务目录：${state.taskDir ?? "未绑定"}\n- 阶段：${phase.name}（${phase.id}）\n- 阶段状态：${phase.status}\n- 阶段门禁：${phase.gateStatus ?? "unknown"}\n- 当前阶段：${state.currentPhaseId ?? "未知"}\n- 当前步骤：${state.currentStepId ?? "未知"}\n- 流程：${state.templateName ?? state.templateId ?? "未知"}${phase.requiredInteractionCount ? `\n- 需交互次数：${phase.requiredInteractionCount}` : ""}${checkText}`

  if (action === "continue") {
    return `${projectPart}：请继续推进任务交付阶段「${phase.name}」。先读取 progress.json 和该阶段相关产物，按项目 workflow 规则推进；如果该阶段需要需求/设计/修复方案确认，请暂停并询问用户；不要直接执行 Git/TAPD 外部交付动作。${context}`
  }
  if (action === "evidence") {
    return `${projectPart}：请补齐任务交付阶段「${phase.name}」的证据。优先检查阶段产物、interaction_log.md、test_cases.md 和 progress.json；只补齐缺失证据或说明未执行原因，不跳过未完成前置阶段，不把缺证据阶段标记为通过。${context}`
  }
  return `${projectPart}：请分析任务交付阶段「${phase.name}」当前阻塞点。请列出阻塞门禁、缺失产物、缺失确认、验证失败或未执行原因，并给出最小恢复步骤；不要直接修改 Git/TAPD 外部状态。${context}`
}

export function buildTaskDeliveryVerificationActionPrompt(
  state: Pick<TaskDeliveryState, "projectName" | "taskDir" | "currentPhaseId" | "templateId" | "templateName">,
  verification: Pick<TaskDeliveryVerificationState, "label" | "command" | "status" | "evidence">,
  action: TaskDeliveryVerificationActionKind,
): string {
  const projectPart = state.projectName ? `项目「${state.projectName}」` : "当前项目"
  const commandText = verification.command ?? verification.label
  const context = `\n\n上下文：\n- 任务目录：${state.taskDir ?? "未绑定"}\n- 当前阶段：${state.currentPhaseId ?? "未知"}\n- 流程：${state.templateName ?? state.templateId ?? "未知"}\n- 验证项：${verification.label}\n- 命令：${commandText}\n- 当前状态：${verification.status}${verification.evidence ? `\n- 已记录证据：${verification.evidence}` : ""}`

  if (action === "run") {
    return `${projectPart}：请为验证项「${verification.label}」生成并执行安全验证指令。先确认命令是否适用于当前任务范围和运行环境；执行后把退出码、关键输出、失败摘要或报告路径记录到 test_cases.md。不要把 Playwright 当作 tsc/build/lint 的替代，不执行 Git/TAPD 外部交付动作。${context}`
  }
  if (action === "skip") {
    return `${projectPart}：请为验证项「${verification.label}」记录未执行原因。先判断该命令是否因环境不可用、任务范围不适用、依赖缺失或已有等价证据而跳过；把原因、风险和后续补验建议写入 test_cases.md，不要将未执行误记为通过。${context}`
  }
  return `${projectPart}：请解释验证项「${verification.label}」的失败或异常状态。读取 test_cases.md、相关报告和最近输出，归纳根因候选、影响范围、最小修复/复验建议；如果证据不足，请明确需要补充哪些日志或命令输出。${context}`
}

export interface TaskDeliveryPhaseState {
  id: string
  name: string
  status: TaskDeliveryItemStatus
  requiredInteractionCount?: number
  startedAt?: string | null
  completedAt?: string | null
  gateStatus?: TaskDeliveryEvidenceStatus
  gateChecks?: TaskDeliveryEvidenceCheck[]
}

export interface TaskDeliveryArtifactState {
  id: string
  name: string
  path?: string | null
  status: TaskDeliveryArtifactStatus
  sizeBytes?: number | null
  modifiedMs?: number | null
  evidenceChecks?: TaskDeliveryEvidenceCheck[]
}

export interface TaskDeliveryVerificationState {
  id: string
  label: string
  command?: string | null
  status: TaskDeliveryVerificationStatus
  evidence?: string | null
}

export interface TaskDeliveryState {
  id: string
  title: string
  source: TaskDeliverySource
  sourceLabel: string
  projectName?: string | null
  templateId?: string | null
  templateName?: string | null
  mode?: string | null
  summary: string
  taskDir?: string | null
  taskStatus?: string | null
  currentPhaseId?: string | null
  currentStepId?: string | null
  updatedAt?: string | null
  nextActions: string[]
  phases: TaskDeliveryPhaseState[]
  artifacts: TaskDeliveryArtifactState[]
  verification: TaskDeliveryVerificationState[]
  actions: TaskDeliveryActionState[]
  gateContracts: TaskDeliveryGateContract[]
  missingContractFiles: string[]
  notes: string[]
}

export interface TaskDeliveryTaskCandidate {
  taskDir: string
  label: string
  taskId?: string | null
  taskTitle?: string | null
  status?: string | null
  currentPhaseId?: string | null
  updatedAt?: string | null
  modifiedMs?: number | null
}

export interface TaskDeliveryProgressPhase {
  id?: string
  phase_id?: string
  name?: string
  status?: string
  started_at?: string | null
  completed_at?: string | null
  outputs?: string[]
  artifacts?: string[]
}

export interface TaskDeliveryProgressJson {
  task_id?: string
  task_title?: string
  workflow_id?: string
  status?: string
  current_phase_id?: string
  current_step_id?: string
  updated_at?: string
  next_actions?: unknown
  phases?: TaskDeliveryProgressPhase[]
}

export interface TaskDeliveryInstanceData {
  taskDir: string
  progress?: TaskDeliveryProgressJson | null
  artifactEntries?: Map<string, { sizeBytes?: number | null; modifiedMs?: number | null }>
  artifactContents?: Map<string, string>
}

const ARTIFACT_REQUIRED_TERMS: Record<string, string[]> = {
  "requirement.md": ["验收", "范围"],
  "design.md": ["项目矩阵", "影响范围", "验证"],
  "key_code.md": ["文件", "变更"],
  "test_cases.md": ["验证", "命令"],
  "issue_log.md": ["问题", "结论"],
  "summary.md": ["验证", "风险"],
  "interaction_log.md": ["时间", "阶段", "结论"],
}

const VERIFICATION_RESULT_TERMS = [
  { status: "passed" as const, terms: ["通过", "passed", "pass", "退出码 0", "exit code 0"] },
  { status: "failed" as const, terms: ["失败", "failed", "fail", "阻塞", "error", "非 0", "non-zero"] },
  { status: "skipped" as const, terms: ["未执行", "跳过", "skipped", "不可用", "未运行"] },
]

const STANDARD_ARTIFACT_NAMES = [
  "index.md",
  "requirement.md",
  "design.md",
  "key_code.md",
  "test_cases.md",
  "issue_log.md",
  "summary.md",
  "progress.json",
  "progress.md",
  "progress.html",
  "interaction_log.md",
]

const DEFAULT_VERIFICATION_COMMANDS: ProjectWorkflowVerificationCommand[] = [
  { id: "tsc", command: "yarn tsc", sourceFiles: [] },
  { id: "build", command: "yarn build:all", sourceFiles: [] },
  { id: "lint", command: "yarn lint", sourceFiles: [] },
  { id: "format", command: "yarn prettier:check", sourceFiles: [] },
  { id: "e2e", command: "yarn test:e2e", sourceFiles: [] },
]

function buildDeliveryActions(project: ProjectMeta | null | undefined, instance?: TaskDeliveryInstanceData | null): TaskDeliveryActionState[] {
  const hasProject = !!project
  const hasTaskDir = !!instance?.taskDir
  const missingProjectReason = "当前会话未绑定项目。"
  const missingTaskDirReason = "尚未绑定任务目录。"
  return [
    {
      id: "start",
      label: "启动",
      description: "生成启动项目 workflow 的安全对话指令。",
      enabled: hasProject,
      disabledReason: hasProject ? null : missingProjectReason,
    },
    {
      id: "continue",
      label: "继续",
      description: "从当前 taskDir 继续推进交付流程。",
      enabled: hasProject && hasTaskDir,
      disabledReason: !hasProject ? missingProjectReason : hasTaskDir ? null : missingTaskDirReason,
    },
    {
      id: "verify",
      label: "验证",
      description: "生成验证阶段指令，由对话流程承接。",
      enabled: hasProject && hasTaskDir,
      disabledReason: !hasProject ? missingProjectReason : hasTaskDir ? null : missingTaskDirReason,
    },
    {
      id: "close",
      label: "收口",
      description: "检查资产和风险，准备交付清单。",
      enabled: hasProject && hasTaskDir,
      disabledReason: !hasProject ? missingProjectReason : hasTaskDir ? null : missingTaskDirReason,
    },
  ]
}

const BUILT_IN_PHASES: TaskDeliveryPhaseState[] = [
  { id: "understand", name: "理解任务", status: "active" },
  { id: "implement", name: "实现变更", status: "pending" },
  { id: "verify", name: "验证结果", status: "pending" },
  { id: "summarize", name: "总结交付", status: "pending" },
]

const BUILT_IN_ARTIFACTS: TaskDeliveryArtifactState[] = [
  { id: "changed-files", name: "修改文件", status: "expected" },
  { id: "verification-evidence", name: "验证证据", status: "expected" },
  { id: "final-summary", name: "交付总结", status: "expected" },
]

const BUILT_IN_VERIFICATION: TaskDeliveryVerificationState[] = [
  { id: "typecheck", label: "Type / syntax check", status: "not_run" },
  { id: "test", label: "Relevant tests", status: "not_run" },
  { id: "manual-review", label: "Manual review / residual risk", status: "not_run" },
]

function normalizeArtifact(
  artifact: ProjectWorkflowFixedArtifact,
  instance?: TaskDeliveryInstanceData | null,
): TaskDeliveryArtifactState {
  const name = artifact.name || artifact.id
  const entry = instance?.artifactEntries?.get(name)
  const content = instance?.artifactContents?.get(name)
  const evidenceChecks = buildArtifactEvidenceChecks(name, content, !!entry, artifact.requiredTerms)
  return {
    id: artifact.id || artifact.name,
    name,
    path: instance?.taskDir ? `${instance.taskDir}/${name}` : artifact.path,
    status: evidenceChecks.some((check) => check.status === "failed")
      ? "invalid"
      : entry
        ? content !== undefined && !content.trim()
          ? "draft"
          : "valid"
        : instance
          ? "missing"
          : "expected",
    sizeBytes: entry?.sizeBytes,
    modifiedMs: entry?.modifiedMs,
    evidenceChecks,
  }
}

function normalizeVerification(
  command: ProjectWorkflowVerificationCommand,
  instance?: TaskDeliveryInstanceData | null,
): TaskDeliveryVerificationState {
  const evidence = extractVerificationEvidence(command.command, instance?.artifactContents?.get("test_cases.md"))
  return {
    id: command.id || command.command,
    label: command.label || command.id || command.command,
    command: command.command,
    status: evidence?.status ?? "not_run",
    evidence: evidence?.detail,
  }
}

function buildArtifactEvidenceChecks(
  name: string,
  content: string | undefined,
  exists: boolean,
  contractTerms?: string[] | null,
): TaskDeliveryEvidenceCheck[] {
  if (!exists) return []
  const requiredTerms = contractTerms?.length ? contractTerms : ARTIFACT_REQUIRED_TERMS[name] ?? []
  if (requiredTerms.length === 0) return []

  const normalizedContent = content?.toLowerCase() ?? ""
  return requiredTerms.map((term) => ({
    id: `${name}:${term}`,
    label: `包含「${term}」`,
    status: normalizedContent.includes(term.toLowerCase()) ? "passed" : "failed",
    detail: normalizedContent.includes(term.toLowerCase()) ? null : `产物缺少必要证据词：${term}`,
  }))
}

function extractVerificationEvidence(
  command: string | null | undefined,
  testCasesContent: string | undefined,
): { status: TaskDeliveryVerificationStatus; detail: string } | null {
  if (!command || !testCasesContent?.trim()) return null

  const commandKey = command.trim().toLowerCase()
  const tableRow = parseMarkdownTableRows(testCasesContent).find((row) =>
    row.some((cell) => cell.toLowerCase().includes(commandKey)),
  )
  const matchedText =
    tableRow?.join(" | ") ?? testCasesContent.split(/\r?\n/).find((line) => line.toLowerCase().includes(commandKey))
  if (!matchedText) return null

  return classifyVerificationEvidence(matchedText)
}

function parseMarkdownTableRows(content: string): string[][] {
  return content
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.startsWith("|") && line.endsWith("|"))
    .filter((line) => !/^\|\s*:?-{3,}:?\s*(\|\s*:?-{3,}:?\s*)+\|$/.test(line))
    .map((line) => line.slice(1, -1).split("|").map((cell) => cell.trim()))
}

function classifyVerificationEvidence(text: string): { status: TaskDeliveryVerificationStatus; detail: string } {
  const normalizedLine = text.toLowerCase()
  const statusOrder = ["failed", "skipped", "passed"] as const
  for (const status of statusOrder) {
    const candidate = VERIFICATION_RESULT_TERMS.find((item) => item.status === status)
    if (candidate?.terms.some((term) => normalizedLine.includes(term.toLowerCase()))) {
      return { status: candidate.status, detail: text.trim() }
    }
  }

  return { status: "unknown", detail: text.trim() }
}

function buildPhaseGateChecks(
  phase: TaskDeliveryProgressPhase | undefined,
  instance?: TaskDeliveryInstanceData | null,
  structuredContracts?: ProjectWorkflowGateContract[] | null,
): TaskDeliveryEvidenceCheck[] {
  const requiredArtifacts = Array.from(new Set([...(phase?.outputs ?? []), ...(phase?.artifacts ?? [])])).filter(Boolean)
  const phaseId = phase?.id ?? phase?.phase_id ?? null
  const phaseContracts = (structuredContracts ?? []).filter((contract) => !contract.phaseId || contract.phaseId === phaseId)
  if (requiredArtifacts.length === 0 && phaseContracts.length === 0) return []
  if (!instance) return []

  const artifactChecks = requiredArtifacts.map((artifactName) => {
    const exists = instance.artifactEntries?.has(artifactName) ?? false
    const content = instance.artifactContents?.get(artifactName)
    const contentChecks = buildArtifactEvidenceChecks(artifactName, content, exists)
    const hasFailedContent = contentChecks.some((check) => check.status === "failed")
    return {
      id: `${phase?.id ?? phase?.phase_id ?? "phase"}:${artifactName}`,
      label: artifactName,
      status: exists && !hasFailedContent ? "passed" : "failed",
      detail: exists ? (hasFailedContent ? "产物存在，但缺少必要证据词。" : "产物存在且基础证据检查通过。") : "阶段要求产物缺失。",
    } satisfies TaskDeliveryEvidenceCheck
  })

  const contractChecks = phaseContracts.flatMap((contract) => {
    const artifacts = contract.requiredArtifacts?.length ? contract.requiredArtifacts : []
    return artifacts.map((artifactName) => {
      const exists = instance.artifactEntries?.has(artifactName) ?? false
      const content = instance.artifactContents?.get(artifactName)
      const contentChecks = buildArtifactEvidenceChecks(artifactName, content, exists, contract.requiredTerms)
      const hasFailedContent = contentChecks.some((check) => check.status === "failed")
      return {
        id: `${contract.id}:${artifactName}`,
        label: `${contract.label}: ${artifactName}`,
        status: exists && !hasFailedContent ? "passed" : "failed",
        detail: exists ? (hasFailedContent ? "结构化门禁要求的证据词缺失。" : "结构化门禁检查通过。") : "结构化门禁要求产物缺失。",
      } satisfies TaskDeliveryEvidenceCheck
    })
  })

  return [...artifactChecks, ...contractChecks]
}

function summarizeGateStatus(checks: TaskDeliveryEvidenceCheck[]): TaskDeliveryEvidenceStatus | undefined {
  if (checks.length === 0) return undefined
  if (checks.some((check) => check.status === "failed")) return "failed"
  if (checks.some((check) => check.status === "warning" || check.status === "unknown")) return "warning"
  return "passed"
}

function artifactFromName(name: string, instance?: TaskDeliveryInstanceData | null): TaskDeliveryArtifactState {
  return normalizeArtifact({ id: name, name, path: name, requiredTerms: [], sourceFiles: [] }, instance)
}

function normalizeGateContract(contract: ProjectWorkflowGateContract): TaskDeliveryGateContract {
  return {
    id: contract.id,
    label: contract.label || contract.id,
    phaseId: contract.phaseId,
    requiredArtifacts: contract.requiredArtifacts ?? [],
    requiredTerms: contract.requiredTerms ?? [],
    source: contract.sourceFiles?.join(", ") || contract.kind || ".agent-workflows",
  }
}

function artifactRequiredTermsFromContracts(
  artifact: ProjectWorkflowFixedArtifact,
  contracts?: ProjectWorkflowGateContract[] | null,
): string[] {
  const names = new Set([artifact.id, artifact.name, artifact.path].filter(Boolean))
  return Array.from(
    new Set([
      ...(artifact.requiredTerms ?? []),
      ...((contracts ?? [])
        .filter((contract) => (contract.requiredArtifacts ?? []).some((name) => names.has(name)))
        .flatMap((contract) => contract.requiredTerms ?? [])),
    ]),
  )
}

function normalizeArtifactWithContracts(
  artifact: ProjectWorkflowFixedArtifact,
  instance?: TaskDeliveryInstanceData | null,
  contracts?: ProjectWorkflowGateContract[] | null,
): TaskDeliveryArtifactState {
  return normalizeArtifact(
    {
      ...artifact,
      requiredTerms: artifactRequiredTermsFromContracts(artifact, contracts),
    },
    instance,
  )
}

function buildProgressPhases(
  progress: TaskDeliveryProgressJson | undefined,
  instance?: TaskDeliveryInstanceData | null,
  taskSnapshot?: TaskProgressSnapshot | null,
): TaskDeliveryPhaseState[] {
  const phases = progress?.phases ?? []
  if (phases.length === 0) {
    return BUILT_IN_PHASES.map((phase, index) => ({ ...phase, status: phaseStatus(index, taskSnapshot) }))
  }

  return phases.map((phase, index) => {
    const id = phase.id ?? phase.phase_id ?? `phase:${index + 1}`
    const gateChecks = buildPhaseGateChecks(phase, instance)
    return {
      id,
      name: phase.name || id,
      status: normalizeProgressStatus(phase.status) ?? (progress?.current_phase_id === id ? "active" : phaseStatus(index, taskSnapshot)),
      startedAt: phase.started_at,
      completedAt: phase.completed_at,
      gateStatus: summarizeGateStatus(gateChecks),
      gateChecks,
    }
  })
}

function buildInstanceArtifacts(instance?: TaskDeliveryInstanceData | null): TaskDeliveryArtifactState[] {
  const names = new Set(STANDARD_ARTIFACT_NAMES)
  for (const phase of instance?.progress?.phases ?? []) {
    for (const name of [...(phase.outputs ?? []), ...(phase.artifacts ?? [])]) names.add(name)
  }
  for (const name of instance?.artifactEntries?.keys() ?? []) {
    if (name.endsWith(".md") || name === "progress.json" || name === "progress.html") names.add(name)
  }
  return Array.from(names).map((name) => artifactFromName(name, instance))
}

function buildGateContracts(
  instance?: TaskDeliveryInstanceData | null,
  structuredContracts?: ProjectWorkflowGateContract[] | null,
): TaskDeliveryGateContract[] {
  const contracts: TaskDeliveryGateContract[] = []
  for (const contract of structuredContracts ?? []) {
    contracts.push(normalizeGateContract(contract))
  }
  for (const phase of instance?.progress?.phases ?? []) {
    const phaseId = phase.id ?? phase.phase_id ?? null
    const artifacts = Array.from(new Set([...(phase.outputs ?? []), ...(phase.artifacts ?? [])])).filter(Boolean)
    if (artifacts.length > 0) {
      contracts.push({
        id: `${phaseId ?? "phase"}:artifacts`,
        label: `${phase.name || phaseId || "阶段"} required artifacts`,
        phaseId,
        requiredArtifacts: artifacts,
        source: "progress.json",
      })
    }
  }
  const structuredArtifactTerms = new Set(
    contracts.flatMap((contract) => contract.requiredArtifacts ?? []),
  )
  for (const [artifactName, terms] of Object.entries(ARTIFACT_REQUIRED_TERMS)) {
    if (structuredArtifactTerms.has(artifactName)) continue
    contracts.push({
      id: `${artifactName}:required_terms`,
      label: `${artifactName} required terms`,
      requiredArtifacts: [artifactName],
      requiredTerms: terms,
      source: "built-in fallback",
    })
  }
  return contracts
}

function firstTemplate(discovery: ProjectWorkflowDiscovery | null): ProjectWorkflowTemplateSummary | null {
  return discovery?.templates?.[0] ?? null
}

function phaseStatus(index: number, taskSnapshot?: TaskProgressSnapshot | null): TaskDeliveryItemStatus {
  if (!taskSnapshot || taskSnapshot.total === 0) return index === 0 ? "active" : "pending"
  if (taskSnapshot.completed === taskSnapshot.total) return "completed"
  if (index < taskSnapshot.completed) return "completed"
  return index === taskSnapshot.completed ? "active" : "pending"
}

function normalizeProgressStatus(status?: string | null): TaskDeliveryItemStatus | null {
  const value = status?.toLowerCase()
  if (!value) return null
  if (["completed", "complete", "pass", "passed", "done"].includes(value)) return "completed"
  if (["blocked", "failed", "fail"].includes(value)) return "blocked"
  if (["skipped", "skip"].includes(value)) return "skipped"
  if (["running", "in_progress", "active", "started"].includes(value)) return "active"
  if (["pending", "todo"].includes(value)) return "pending"
  return "unknown"
}

function normalizeNextActions(value: unknown): string[] {
  if (Array.isArray(value)) return value.map((item) => String(item)).filter(Boolean)
  if (typeof value === "string" && value.trim()) return [value.trim()]
  return []
}

export function buildTaskDeliveryState(input: {
  project?: ProjectMeta | null
  discovery?: ProjectWorkflowDiscovery | null
  preview?: ProjectWorkflowPreview | null
  loading?: boolean
  error?: string | null
  taskSnapshot?: TaskProgressSnapshot | null
  instance?: TaskDeliveryInstanceData | null
}): TaskDeliveryState {
  const { project, discovery, preview, loading, error, taskSnapshot, instance } = input
  const template = firstTemplate(discovery ?? null)
  const progress = instance?.progress
  const progressPhaseById = new Map(
    (progress?.phases ?? [])
      .map((phase) => [phase.id ?? phase.phase_id, phase] as const)
      .filter(([id]) => !!id) as Array<[string, TaskDeliveryProgressPhase]>,
  )

  if (discovery?.exists && template) {
    const previewGateContracts = preview?.gateContracts ?? []
    const discoveryGateContracts = discovery.gateContracts ?? []
    const structuredGateContracts = previewGateContracts.length ? previewGateContracts : discoveryGateContracts
    const previewArtifacts = preview?.fixedArtifacts ?? []
    const discoveryArtifacts = discovery.fixedArtifacts ?? []
    const previewVerificationCommands = preview?.verificationCommands ?? []
    const discoveryVerificationCommands = discovery.verificationCommands ?? []
    const phases =
      preview?.phases.map((phase, index) => {
        const progressPhase = progressPhaseById.get(phase.id)
        const phaseContracts = structuredGateContracts.filter((contract) => !contract.phaseId || contract.phaseId === phase.id)
        const phaseForGateChecks = progressPhase ?? { id: phase.id, name: phase.name }
        const gateChecks = buildPhaseGateChecks(phaseForGateChecks, instance, phaseContracts)
        return {
          id: phase.id,
          name: phase.name || phase.id,
          status:
            normalizeProgressStatus(progressPhase?.status) ??
            (progress?.current_phase_id === phase.id ? "active" : phaseStatus(index, taskSnapshot)),
          requiredInteractionCount: phase.requiredInteractions.length,
          startedAt: progressPhase?.started_at,
          completedAt: progressPhase?.completed_at,
          gateStatus: summarizeGateStatus(gateChecks),
          gateChecks,
        }
      }) ??
      (progress ? buildProgressPhases(progress, instance, taskSnapshot) :
        Array.from({ length: template.phaseCount }, (_, index) => ({
          id: `${template.id}:phase:${index + 1}`,
          name: `阶段 ${index + 1}`,
          status: phaseStatus(index, taskSnapshot),
        })))

    return {
      id: `project-workflow:${project?.id ?? discovery.projectId}:${template.id}`,
      title: "Task Delivery",
      source: "project-workflow",
      sourceLabel: ".agent-workflows",
      projectName: project?.name,
      templateId: template.id,
      templateName: template.name || template.id,
      mode: preview?.mode ?? template.modes[0] ?? null,
      summary: loading
        ? "正在读取项目交付契约…"
        : instance
          ? "已绑定任务目录，正在展示 progress.json 与固定产物实例状态。"
          : "使用项目 .agent-workflows 定义的阶段、产物和验证契约。",
      taskDir: instance?.taskDir,
      taskStatus: progress?.status,
      currentPhaseId: progress?.current_phase_id,
      currentStepId: progress?.current_step_id,
      updatedAt: progress?.updated_at,
      nextActions: normalizeNextActions(progress?.next_actions),
      phases,
      artifacts: (previewArtifacts.length || discoveryArtifacts.length)
        ? (previewArtifacts.length ? previewArtifacts : discoveryArtifacts).map((artifact) =>
          normalizeArtifactWithContracts(artifact, instance, structuredGateContracts),
        )
        : buildInstanceArtifacts(instance),
      verification: (previewVerificationCommands.length || discoveryVerificationCommands.length)
        ? (previewVerificationCommands.length ? previewVerificationCommands : discoveryVerificationCommands).map((command) =>
          normalizeVerification(command, instance),
        )
        : DEFAULT_VERIFICATION_COMMANDS.map((command) => normalizeVerification(command, instance)),
      actions: buildDeliveryActions(project, instance),
      gateContracts: buildGateContracts(
        instance,
        structuredGateContracts,
      ),
      missingContractFiles: discovery.missingFiles ?? [],
      notes: [
        "这是标准化 Task Delivery 视图；.agent-workflows 只是当前数据来源。",
        instance
          ? "阶段状态优先使用 progress.json；产物状态来自任务目录文件扫描，并对关键 Markdown 产物做基础证据词检查。"
          : "尚未发现任务目录时展示契约模板；创建 docs/tasks/**/progress.json 后会自动绑定最近任务。",
        ...(error ? [`项目交付契约读取失败：${error}`] : []),
      ],
    }
  }

  if (progress) {
    return {
      id: `task-instance:${project?.id ?? "project"}:${instance.taskDir}`,
      title: "Task Delivery",
      source: project ? "built-in" : "none",
      sourceLabel: "Task instance",
      projectName: project?.name,
      templateId: progress?.workflow_id,
      templateName: progress?.workflow_id ?? "progress.json",
      summary: "已发现任务目录；即使项目契约不完整，也优先按 progress.json 展示真实交付状态。",
      taskDir: instance.taskDir,
      taskStatus: progress.status,
      currentPhaseId: progress.current_phase_id,
      currentStepId: progress.current_step_id,
      updatedAt: progress.updated_at,
      nextActions: normalizeNextActions(progress.next_actions),
      phases: buildProgressPhases(progress, instance, taskSnapshot),
      artifacts: buildInstanceArtifacts(instance),
      verification: DEFAULT_VERIFICATION_COMMANDS.map((command) => normalizeVerification(command, instance)),
      actions: buildDeliveryActions(project, instance),
      gateContracts: buildGateContracts(instance),
      missingContractFiles: discovery?.missingFiles ?? [],
      notes: [
        "当前展示由任务实例 progress.json 驱动，不依赖完整 .agent-workflows 契约。",
        ...(error ? [`项目交付契约读取失败：${error}`] : []),
      ],
    }
  }

  const source: TaskDeliverySource = project ? "built-in" : "none"
  return {
    id: `built-in:${project?.id ?? "session"}`,
    title: "Task Delivery",
    source,
    sourceLabel: project ? "Built-in fallback" : "Session fallback",
    projectName: project?.name,
    summary: loading
      ? "正在检查项目是否提供交付契约…"
      : project
        ? "当前项目未提供 .agent-workflows，使用内置通用交付流程。"
        : "当前会话未绑定项目，使用最小会话交付流程。",
    taskDir: instance?.taskDir,
    taskStatus: undefined,
    currentPhaseId: undefined,
    currentStepId: undefined,
    updatedAt: undefined,
    nextActions: [],
    phases: BUILT_IN_PHASES.map((phase, index) => ({ ...phase, status: phaseStatus(index, taskSnapshot) })),
    artifacts: BUILT_IN_ARTIFACTS,
    verification: BUILT_IN_VERIFICATION,
    actions: buildDeliveryActions(project, instance),
    gateContracts: buildGateContracts(instance),
    missingContractFiles: discovery?.missingFiles ?? [],
    notes: [
      "没有项目契约时不强制生成固定文件，只保留修改文件、验证证据和总结交付。",
      ...(error ? [`项目交付契约读取失败：${error}`] : []),
    ],
  }
}
