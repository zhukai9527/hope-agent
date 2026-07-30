import { AlertCircle, CheckCircle2, Circle, ClipboardCheck, ExternalLink, FileText, Loader2, Play, RefreshCw, Search, ShieldCheck, Wrench, X } from "lucide-react"
import { cn } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import type {
  TaskDeliveryArtifactState,
  TaskDeliveryEvidenceStatus,
  TaskDeliveryItemStatus,
  TaskDeliveryActionKind,
  TaskDeliveryPhaseActionKind,
  TaskDeliveryPhaseState,
  TaskDeliveryState,
  TaskDeliveryTaskCandidate,
  TaskDeliveryVerificationActionKind,
  TaskDeliveryVerificationState,
} from "./taskDelivery"

interface TaskDeliveryPanelProps {
  state: TaskDeliveryState
  loading?: boolean
  taskCandidates?: TaskDeliveryTaskCandidate[]
  selectedTaskDir?: string | null
  onSelectTaskDir?: (taskDir: string | null) => void
  onAction?: (action: TaskDeliveryActionKind) => void
  onPhaseAction?: (phase: TaskDeliveryPhaseState, action: TaskDeliveryPhaseActionKind) => void
  onOpenArtifact?: (artifact: TaskDeliveryArtifactState) => void
  onRepairArtifact?: (artifact: TaskDeliveryArtifactState) => void
  onVerificationAction?: (verification: TaskDeliveryVerificationState, action: TaskDeliveryVerificationActionKind) => void
  onRefresh?: () => void
  onClose?: () => void
}

const statusClass: Record<TaskDeliveryItemStatus, string> = {
  pending: "border-border text-muted-foreground",
  active: "border-blue-500/60 bg-blue-500/10 text-blue-600 dark:text-blue-300",
  blocked: "border-destructive/60 bg-destructive/10 text-destructive",
  completed: "border-emerald-500/60 bg-emerald-500/10 text-emerald-600 dark:text-emerald-300",
  skipped: "border-muted bg-muted/60 text-muted-foreground",
  unknown: "border-border text-muted-foreground",
}

function StatusDot({ status }: { status: TaskDeliveryItemStatus }) {
  if (status === "completed") return <CheckCircle2 className="h-4 w-4 text-emerald-500" />
  if (status === "active") return <Loader2 className="h-4 w-4 animate-spin text-blue-500" />
  if (status === "blocked") return <AlertCircle className="h-4 w-4 text-destructive" />
  return <Circle className="h-4 w-4 text-muted-foreground/70" />
}

function artifactTone(artifact: TaskDeliveryArtifactState) {
  if (artifact.status === "valid") return "text-emerald-600 dark:text-emerald-300"
  if (artifact.status === "invalid" || artifact.status === "missing") return "text-destructive"
  return "text-muted-foreground"
}

function verificationTone(item: TaskDeliveryVerificationState) {
  if (item.status === "passed") return "text-emerald-600 dark:text-emerald-300"
  if (item.status === "failed") return "text-destructive"
  if (item.status === "running") return "text-blue-600 dark:text-blue-300"
  return "text-muted-foreground"
}

function evidenceTone(status?: TaskDeliveryEvidenceStatus) {
  if (status === "passed") return "text-emerald-600 dark:text-emerald-300"
  if (status === "failed") return "text-destructive"
  if (status === "warning") return "text-amber-600 dark:text-amber-300"
  return "text-muted-foreground"
}

function evidenceLabel(status?: TaskDeliveryEvidenceStatus) {
  if (status === "passed") return "gate passed"
  if (status === "failed") return "gate failed"
  if (status === "warning") return "gate warning"
  return "gate unknown"
}

export default function TaskDeliveryPanel({
  state,
  loading = false,
  taskCandidates = [],
  selectedTaskDir,
  onSelectTaskDir,
  onAction,
  onPhaseAction,
  onOpenArtifact,
  onRepairArtifact,
  onVerificationAction,
  onRefresh,
  onClose,
}: TaskDeliveryPanelProps) {
  const completedPhases = state.phases.filter((phase) => phase.status === "completed").length
  const totalPhases = state.phases.length
  const progress = totalPhases > 0 ? Math.round((completedPhases / totalPhases) * 100) : 0

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden rounded-2xl border border-border/70 bg-surface-floating shadow-sm">
      <header className="flex shrink-0 items-start gap-3 border-b border-border/70 px-4 py-3">
        <div className="mt-0.5 flex h-9 w-9 shrink-0 items-center justify-center rounded-xl bg-blue-500/10 text-blue-600 dark:text-blue-300">
          <ClipboardCheck className="h-5 w-5" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <h2 className="truncate text-sm font-semibold text-foreground">{state.title}</h2>
            <span className="rounded-full border border-border/70 px-2 py-0.5 text-[11px] text-muted-foreground">
              {state.sourceLabel}
            </span>
          </div>
          <p className="mt-1 line-clamp-2 text-xs leading-5 text-muted-foreground">{state.summary}</p>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          {onRefresh && (
            <Button variant="ghost" size="icon" className="h-8 w-8" onClick={onRefresh} disabled={loading} title="刷新交付状态">
              <RefreshCw className={cn("h-4 w-4", loading && "animate-spin")} />
            </Button>
          )}
          {onClose && (
            <Button variant="ghost" size="icon" className="h-8 w-8" onClick={onClose}>
              <X className="h-4 w-4" />
            </Button>
          )}
        </div>
      </header>

      <div className="min-h-0 flex-1 space-y-4 overflow-y-auto px-4 py-4">
        <section className="rounded-xl border border-border/70 bg-card/70 p-3">
          <div className="flex items-center justify-between gap-3 text-xs">
            <div className="min-w-0">
              <div className="font-medium text-foreground">{state.templateName ?? state.projectName ?? "通用交付流程"}</div>
              <div className="mt-1 truncate text-muted-foreground">
                {state.projectName ? `Project: ${state.projectName}` : "未绑定项目"}
                {state.mode ? ` · Mode: ${state.mode}` : ""}
              </div>
            </div>
            <div className="shrink-0 text-right text-muted-foreground">
              <div className="text-sm font-semibold text-foreground">{progress}%</div>
              <div>{completedPhases}/{totalPhases} phases</div>
            </div>
          </div>
          <div className="mt-3 h-1.5 overflow-hidden rounded-full bg-muted">
            <div className="h-full rounded-full bg-blue-500 transition-[width]" style={{ width: `${progress}%` }} />
          </div>
        </section>

        {(state.taskDir || state.currentPhaseId || state.nextActions.length > 0) && (
          <section className="rounded-xl border border-border/70 bg-card/70 p-3 text-xs">
            <div className="mb-2 flex items-center justify-between gap-2">
              <h3 className="font-semibold uppercase tracking-wide text-muted-foreground">当前任务实例</h3>
              {taskCandidates.length > 1 && onSelectTaskDir && (
                <select
                  className="min-w-0 max-w-[55%] rounded-md border border-border bg-background px-2 py-1 text-[11px] text-foreground outline-none"
                  value={selectedTaskDir ?? state.taskDir ?? ""}
                  onChange={(event) => onSelectTaskDir(event.target.value || null)}
                  title="选择任务目录"
                >
                  {taskCandidates.map((candidate) => (
                    <option key={candidate.taskDir} value={candidate.taskDir}>
                      {candidate.label}
                    </option>
                  ))}
                </select>
              )}
            </div>
            <div className="space-y-1.5 text-muted-foreground">
              {state.taskDir && (
                <div className="flex gap-2">
                  <span className="shrink-0 text-foreground">任务目录</span>
                  <span className="min-w-0 flex-1 break-all font-mono">{state.taskDir}</span>
                </div>
              )}
              {state.taskStatus && (
                <div className="flex gap-2">
                  <span className="shrink-0 text-foreground">状态</span>
                  <span>{state.taskStatus}</span>
                </div>
              )}
              {state.currentPhaseId && (
                <div className="flex gap-2">
                  <span className="shrink-0 text-foreground">当前阶段</span>
                  <span className="font-mono">{state.currentPhaseId}</span>
                </div>
              )}
              {state.currentStepId && (
                <div className="flex gap-2">
                  <span className="shrink-0 text-foreground">当前步骤</span>
                  <span className="font-mono">{state.currentStepId}</span>
                </div>
              )}
              {state.updatedAt && (
                <div className="flex gap-2">
                  <span className="shrink-0 text-foreground">更新时间</span>
                  <span>{state.updatedAt}</span>
                </div>
              )}
            </div>
            {state.nextActions.length > 0 && (
              <div className="mt-3">
                <div className="mb-1 font-medium text-foreground">下一步</div>
                <ul className="list-disc space-y-1 pl-4 text-muted-foreground">
                  {state.nextActions.map((action) => (
                    <li key={action}>{action}</li>
                  ))}
                </ul>
              </div>
            )}
          </section>
        )}

        <section className="rounded-xl border border-border/70 bg-card/70 p-3 text-xs">
          <h3 className="mb-2 font-semibold uppercase tracking-wide text-muted-foreground">交付动作</h3>
          <div className="grid grid-cols-2 gap-2">
            {state.actions.map((action) => (
              <Button
                key={action.id}
                variant="outline"
                size="sm"
                className="h-auto justify-start px-2 py-2 text-left"
                disabled={!action.enabled || !onAction}
                title={!onAction ? "当前面板未接入对话发送入口。" : action.disabledReason ?? action.description}
                onClick={() => onAction?.(action.id)}
              >
                <span className="min-w-0">
                  <span className="block text-xs font-medium">{action.label}</span>
                  <span className="line-clamp-2 text-[11px] font-normal text-muted-foreground">{action.description}</span>
                  {(action.disabledReason || !onAction) && (
                    <span className="mt-1 block line-clamp-1 text-[10px] font-normal text-muted-foreground/80">
                      {!onAction ? "未接入对话发送入口" : action.disabledReason}
                    </span>
                  )}
                </span>
              </Button>
            ))}
          </div>
          <p className="mt-2 text-[11px] text-muted-foreground">
            点击后只会向当前对话发送项目 workflow 指令；Git/TAPD 回写和验证命令仍由对话流程确认后执行。
          </p>
        </section>

        <section>
          <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">流程步骤</h3>
          <ol className="space-y-2">
            {state.phases.map((phase, index) => (
              <li key={phase.id} className={cn("rounded-xl border px-3 py-2", statusClass[phase.status])}>
                <div className="flex items-start gap-2">
                  <StatusDot status={phase.status} />
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2 text-sm font-medium">
                      <span className="shrink-0 tabular-nums text-muted-foreground">{index + 1}.</span>
                      <span className="truncate text-foreground">{phase.name}</span>
                    </div>
                    <div className="mt-1 flex flex-wrap gap-1 text-[11px] text-muted-foreground">
                      <span>{phase.id}</span>
                      {!!phase.requiredInteractionCount && <span>· 需交互 {phase.requiredInteractionCount}</span>}
                      {phase.gateStatus && <span className={evidenceTone(phase.gateStatus)}>· {evidenceLabel(phase.gateStatus)}</span>}
                    </div>
                    {phase.gateChecks && phase.gateChecks.length > 0 && (
                      <div className="mt-2 space-y-1 border-t border-border/50 pt-2 text-[11px]">
                        {phase.gateChecks.map((check) => (
                          <div key={check.id} className="flex items-start gap-2">
                            <span className={cn("shrink-0", evidenceTone(check.status))}>{check.status}</span>
                            <span className="min-w-0 flex-1 text-muted-foreground">
                              {check.label}{check.detail ? ` · ${check.detail}` : ""}
                            </span>
                          </div>
                        ))}
                      </div>
                    )}
                  </div>
                  <div className="flex shrink-0 items-center gap-1">
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7"
                      disabled={!onPhaseAction}
                      title="继续此阶段"
                      onClick={() => onPhaseAction?.(phase, "continue")}
                    >
                      <Play className="h-3.5 w-3.5" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7"
                      disabled={!onPhaseAction}
                      title="补齐阶段证据"
                      onClick={() => onPhaseAction?.(phase, "evidence")}
                    >
                      <Wrench className="h-3.5 w-3.5" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7"
                      disabled={!onPhaseAction}
                      title="查看阶段阻塞"
                      onClick={() => onPhaseAction?.(phase, "blockers")}
                    >
                      <Search className="h-3.5 w-3.5" />
                    </Button>
                  </div>
                </div>
              </li>
            ))}
          </ol>
        </section>

        <section>
          <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">标准产物</h3>
          <div className="grid gap-2">
            {state.artifacts.map((artifact) => (
              <div key={artifact.id} className="flex items-center gap-2 rounded-lg border border-border/70 bg-card/60 px-3 py-2 text-sm">
                <FileText className={cn("h-4 w-4 shrink-0", artifactTone(artifact))} />
                <div className="min-w-0 flex-1">
                  <div className="truncate text-foreground">{artifact.name}</div>
                  {artifact.path && <div className="truncate text-xs text-muted-foreground">{artifact.path}</div>}
                  {artifact.modifiedMs ? (
                    <div className="truncate text-xs text-muted-foreground">
                      {artifact.sizeBytes ?? 0} bytes · {new Date(artifact.modifiedMs).toLocaleString()}
                    </div>
                  ) : null}
                  {artifact.evidenceChecks && artifact.evidenceChecks.length > 0 && (
                    <div className="mt-1 flex flex-wrap gap-x-2 gap-y-0.5 text-[11px]">
                      {artifact.evidenceChecks.map((check) => (
                        <span key={check.id} className={evidenceTone(check.status)}>
                          {check.status === "passed" ? "✓" : "!"} {check.label}
                        </span>
                      ))}
                    </div>
                  )}
                </div>
                <span className={cn("shrink-0 text-xs", artifactTone(artifact))}>{artifact.status}</span>
                <div className="flex shrink-0 items-center gap-1">
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7"
                    disabled={!artifact.path || !onOpenArtifact}
                    title={artifact.path ? "打开产物" : "产物文件尚不存在"}
                    onClick={() => onOpenArtifact?.(artifact)}
                  >
                    <ExternalLink className="h-3.5 w-3.5" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7"
                    disabled={!onRepairArtifact}
                    title="生成补齐/修复产物指令"
                    onClick={() => onRepairArtifact?.(artifact)}
                  >
                    <Wrench className="h-3.5 w-3.5" />
                  </Button>
                </div>
              </div>
            ))}
            {state.artifacts.length === 0 && (
              <div className="rounded-lg border border-dashed border-border px-3 py-4 text-center text-sm text-muted-foreground">
                暂无固定产物定义。
              </div>
            )}
          </div>
        </section>

        <section>
          <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">验证契约</h3>
          <div className="grid gap-2">
            {state.verification.map((item) => (
              <div key={item.id} className="rounded-lg border border-border/70 bg-card/60 px-3 py-2 text-sm">
                <div className="flex items-center gap-2">
                  <ShieldCheck className={cn("h-4 w-4 shrink-0", verificationTone(item))} />
                  <span className="min-w-0 flex-1 truncate text-foreground">{item.label}</span>
                  <span className={cn("shrink-0 text-xs", verificationTone(item))}>{item.status}</span>
                  <div className="flex shrink-0 items-center gap-1">
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7"
                      disabled={!onVerificationAction}
                      title="生成/执行验证指令"
                      onClick={() => onVerificationAction?.(item, "run")}
                    >
                      <Play className="h-3.5 w-3.5" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7"
                      disabled={!onVerificationAction}
                      title="记录未执行原因"
                      onClick={() => onVerificationAction?.(item, "skip")}
                    >
                      <AlertCircle className="h-3.5 w-3.5" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7"
                      disabled={!onVerificationAction}
                      title="解释失败/异常"
                      onClick={() => onVerificationAction?.(item, "explain")}
                    >
                      <Search className="h-3.5 w-3.5" />
                    </Button>
                  </div>
                </div>
                {item.command && <div className="mt-1 truncate font-mono text-xs text-muted-foreground">{item.command}</div>}
                {item.evidence && <div className="mt-1 line-clamp-2 text-xs text-muted-foreground">{item.evidence}</div>}
              </div>
            ))}
            {state.verification.length === 0 && (
              <div className="rounded-lg border border-dashed border-border px-3 py-4 text-center text-sm text-muted-foreground">
                暂无验证命令定义。
              </div>
            )}
          </div>
        </section>

        {state.gateContracts.length > 0 && (
          <section>
            <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">门禁契约</h3>
            <div className="space-y-2">
              {state.gateContracts.slice(0, 8).map((gate) => (
                <div key={gate.id} className="rounded-lg border border-border/70 bg-card/60 px-3 py-2 text-xs">
                  <div className="truncate font-medium text-foreground">{gate.label}</div>
                  <div className="mt-1 flex flex-wrap gap-x-2 gap-y-1 text-muted-foreground">
                    {gate.phaseId && <span>phase: {gate.phaseId}</span>}
                    {!!gate.requiredArtifacts?.length && <span>artifacts: {gate.requiredArtifacts.join(", ")}</span>}
                    {!!gate.requiredTerms?.length && <span>terms: {gate.requiredTerms.join(", ")}</span>}
                    {gate.source && <span>source: {gate.source}</span>}
                  </div>
                </div>
              ))}
              {state.gateContracts.length > 8 && (
                <div className="text-xs text-muted-foreground">还有 {state.gateContracts.length - 8} 条门禁契约未展开显示。</div>
              )}
            </div>
          </section>
        )}

        {(state.missingContractFiles.length > 0 || state.notes.length > 0 || loading) && (
          <section className="space-y-2 rounded-xl border border-border/70 bg-muted/30 p-3 text-xs text-muted-foreground">
            {loading && <div>正在刷新交付状态…</div>}
            {state.missingContractFiles.length > 0 && (
              <div>
                <div className="font-medium text-foreground">缺失契约文件</div>
                <div className="mt-1 break-words">{state.missingContractFiles.join(", ")}</div>
              </div>
            )}
            {state.notes.map((note) => (
              <p key={note}>{note}</p>
            ))}
          </section>
        )}
      </div>
    </div>
  )
}
