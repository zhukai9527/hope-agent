import { useCallback, useEffect, useState } from "react"
import { useTranslation } from "react-i18next"
import { toast } from "sonner"
import {
  AlertCircle,
  AlertTriangle,
  Brain,
  Check,
  CheckCircle2,
  ChevronDown,
  Loader2,
  RotateCcw,
  Settings2,
  StopCircle,
  Zap,
} from "lucide-react"

import { AnimatedCollapse } from "@/components/ui/animated-presence"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Progress } from "@/components/ui/progress"
import { Switch } from "@/components/ui/switch"
import { getTransport } from "@/lib/transport-provider"
import { cn } from "@/lib/utils"
import { logger } from "@/lib/logger"
import type { ChunkConfig, PassiveRecallConfig } from "@/types/knowledge"
import {
  isLocalModelJobActive,
  isLocalModelJobTerminal,
  phaseTranslationKey,
  type LocalModelJobSnapshot,
} from "@/types/local-model-jobs"
import { useReembedJob } from "@/hooks/useReembedJob"
import {
  embeddingProviderLabel,
  type EmbeddingModelConfig,
  // The wire shape of `knowledge_embedding_get_cmd` is `EmbeddingSelectionState`
  // (generic), serialized identically to the memory one — reuse the TS type.
  type MemoryEmbeddingState as EmbeddingSelectionState,
} from "@/types/embedding-models"
import EmbeddingActivationDialog from "./memory-panel/EmbeddingActivationDialog"
import KnowledgeMaintenanceSection from "./KnowledgeMaintenanceSection"
import SpriteSection from "./SpriteSection"

const EMPTY_STATE: EmbeddingSelectionState = {
  selection: { enabled: false, modelConfigId: null, activeSignature: null, lastReembeddedSignature: null },
  currentModel: null,
  needsReembed: false,
}

/**
 * Knowledge-base vector-search (embedding) settings (D7).
 *
 * Independent of memory: own enable switch / model / signature / reembed
 * lifecycle, but draws from the **shared** `embedding_models` library (managed
 * under Settings → Memory → Embedding models). When the library is empty the
 * activation dialog collapses to a "go configure / download a local model" CTA,
 * reusing memory's onboarding path.
 */
export default function KnowledgePanel() {
  const { t } = useTranslation()
  const [state, setState] = useState<EmbeddingSelectionState>(EMPTY_STATE)
  const [models, setModels] = useState<EmbeddingModelConfig[]>([])
  const [activationOpen, setActivationOpen] = useState(false)

  const reload = useCallback(async () => {
    try {
      const [list, st] = await Promise.all([
        getTransport().call<EmbeddingModelConfig[]>("embedding_model_config_list"),
        getTransport().call<EmbeddingSelectionState>("knowledge_embedding_get_cmd"),
      ])
      setModels(list)
      setState(st)
    } catch (e) {
      logger.error("settings", "KnowledgePanel::reload", "Failed to load knowledge embedding", e)
    }
  }, [])

  // Initial load. setState lives in the async `.then` callback (not the effect
  // body) per react-hooks/set-state-in-effect; `reload` is reused by the reembed
  // hook's onCompleted, where calling it is fine (it's a listener callback).
  useEffect(() => {
    let cancelled = false
    Promise.all([
      getTransport().call<EmbeddingModelConfig[]>("embedding_model_config_list"),
      getTransport().call<EmbeddingSelectionState>("knowledge_embedding_get_cmd"),
    ])
      .then(([list, st]) => {
        if (cancelled) return
        setModels(list)
        setState(st)
      })
      .catch((e) =>
        logger.error("settings", "KnowledgePanel::load", "Failed to load knowledge embedding", e),
      )
    return () => {
      cancelled = true
    }
  }, [])

  // Track the knowledge reembed job via the shared hook (same plumbing as the
  // memory panel; only the kind filter + onCompleted differ). Refresh state on
  // completion so `needsReembed` clears.
  const { job: reembedJob } = useReembedJob({ kind: "knowledge_reembed", onCompleted: reload })

  const activate = useCallback(
    async (modelConfigId: string): Promise<boolean> => {
      try {
        const st = await getTransport().call<EmbeddingSelectionState>(
          "knowledge_embedding_set_default_cmd",
          { modelConfigId },
        )
        setState(st)
        toast.success(t("settings.knowledgeEmbedding.activated"))
        return true
      } catch (e) {
        logger.error("settings", "KnowledgePanel::activate", "Failed to activate", e)
        toast.error(String(e))
        return false
      }
    },
    [t],
  )

  function handleToggle(next: boolean) {
    if (!next) {
      void getTransport()
        .call<EmbeddingSelectionState>("knowledge_embedding_disable_cmd")
        .then(setState)
        .catch((e) => {
          logger.error("settings", "KnowledgePanel::disable", "Failed to disable", e)
          toast.error(String(e))
        })
      return
    }
    const remembered = state.selection.modelConfigId
    const stillValid = remembered && models.some((m) => m.id === remembered)
    if (stillValid) {
      void activate(remembered)
    } else {
      setActivationOpen(true)
    }
  }

  // Force a full rebuild of every KB — bypasses set_default's same-signature
  // short-circuit so the button always works; progress shows in the card below.
  const rebuild = useCallback(async () => {
    try {
      await getTransport().call("knowledge_embedding_rebuild_cmd")
    } catch (e) {
      logger.error("settings", "KnowledgePanel::rebuild", "Failed to start rebuild", e)
      toast.error(String(e))
    }
  }, [])

  const enabled = state.selection.enabled
  const current = state.currentModel

  return (
    <div className="flex-1 overflow-y-auto p-6 space-y-4">
      <div>
        <h2 className="text-lg font-semibold">{t("settings.knowledgeEmbedding.title")}</h2>
        <p className="mt-1 text-sm text-muted-foreground">
          {t("settings.knowledgeEmbedding.description")}
        </p>
      </div>

      <div className="flex items-center justify-between rounded-lg bg-secondary/30 px-3 py-3">
        <div>
          <div className="text-sm font-medium">{t("settings.knowledgeEmbedding.enabled")}</div>
          <div className="text-xs text-muted-foreground">
            {t("settings.knowledgeEmbedding.enabledDesc")}
          </div>
        </div>
        <Switch checked={enabled} onCheckedChange={handleToggle} />
      </div>

      {enabled && current && (
        <div className="flex items-start gap-3 rounded-lg border border-border bg-card px-3 py-2.5">
          <Brain className="mt-0.5 h-4 w-4 shrink-0 text-primary" />
          <div className="min-w-0 flex-1">
            <div className="text-sm font-medium">{current.name}</div>
            <div className="mt-0.5 text-xs text-muted-foreground">
              {embeddingProviderLabel(current)}
              {current.apiModel ? ` · ${current.apiModel}` : ""}
              {current.apiDimensions ? ` · ${current.apiDimensions}d` : ""}
            </div>
          </div>
          <Button variant="outline" size="sm" onClick={() => setActivationOpen(true)}>
            {t("settings.knowledgeEmbedding.changeModel")}
          </Button>
        </div>
      )}

      {enabled && (
        <KnowledgeReembedCard
          job={reembedJob}
          needsReembed={state.needsReembed}
          canRebuild={!!current}
          onRebuild={() => void rebuild()}
        />
      )}

      <ChunkAdvancedSection />

      <PassiveRecallSection />

      <KnowledgeMaintenanceSection />

      <SpriteSection />

      <EmbeddingActivationDialog
        open={activationOpen}
        onOpenChange={setActivationOpen}
        embeddingModels={models}
        onConfirm={activate}
      />
    </div>
  )
}

/**
 * Reembed status for the knowledge index. Progress is KB-granular. Visually
 * parallels memory's `ReembedJobCard` (status icon / progress bar / error /
 * retry) but with a knowledge-specific idle state (needs-rebuild / Rebuild-now)
 * vs memory's manual reembed-all + dismiss. TODO(dedup): if those idle/badge
 * differences converge, extract a shared `<ReembedJobView>`.
 */
function KnowledgeReembedCard({
  job,
  needsReembed,
  canRebuild,
  onRebuild,
}: {
  job: LocalModelJobSnapshot | null
  needsReembed: boolean
  canRebuild: boolean
  onRebuild: () => void
}) {
  const { t } = useTranslation()

  if (!job) {
    return (
      <div className="rounded-lg border border-border bg-card p-4">
        <div className="flex items-center justify-between gap-3">
          <div>
            <div className="text-sm font-medium">{t("settings.knowledgeEmbedding.reembed.title")}</div>
            <div className="mt-0.5 text-xs text-muted-foreground">
              {needsReembed
                ? t("settings.knowledgeEmbedding.reembed.needs")
                : t("settings.knowledgeEmbedding.reembed.idle")}
            </div>
          </div>
          <Button variant="outline" size="sm" disabled={!canRebuild} onClick={onRebuild}>
            <Zap className="mr-1.5 h-3.5 w-3.5" />
            {t("settings.knowledgeEmbedding.reembed.startNow")}
          </Button>
        </div>
      </div>
    )
  }

  const active = isLocalModelJobActive(job)
  const terminal = isLocalModelJobTerminal(job)
  const phaseKey = phaseTranslationKey(job.phase)
  const phaseLabel = phaseKey ? t(phaseKey) : job.phase
  const done = Number(job.bytesCompleted ?? 0)
  const total = Number(job.bytesTotal ?? 0)
  const percent = total > 0 ? Math.min(100, Math.floor((done / total) * 100)) : (job.percent ?? 0)

  return (
    <div className="rounded-lg border border-border bg-card p-4">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2 text-sm font-medium">
            {active ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin text-primary" />
            ) : job.status === "completed" ? (
              <CheckCircle2 className="h-3.5 w-3.5 text-emerald-500" />
            ) : job.status === "failed" || job.status === "interrupted" ? (
              <AlertCircle className="h-3.5 w-3.5 text-destructive" />
            ) : (
              <StopCircle className="h-3.5 w-3.5 text-muted-foreground" />
            )}
            {t("settings.knowledgeEmbedding.reembed.title")}
          </div>
          <div className="mt-1 text-xs text-muted-foreground">
            {job.displayName} · {phaseLabel}
          </div>
        </div>
        {active && (
          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              void getTransport()
                .call("local_model_job_cancel", { jobId: job.jobId })
                .catch((e) => toast.error(String(e)))
            }}
          >
            <StopCircle className="mr-1.5 h-3.5 w-3.5" />
            {t("settings.knowledgeEmbedding.reembed.cancel")}
          </Button>
        )}
      </div>

      {(active || terminal) && (
        <div className="mt-3">
          <Progress value={percent} />
          <div className="mt-1 flex items-center justify-between text-[11px] text-muted-foreground">
            <span>{t("settings.knowledgeEmbedding.reembed.progress", { done, total })}</span>
            <span className="font-mono">{percent}%</span>
          </div>
        </div>
      )}

      {job.error && (
        <div className="mt-3 flex items-start gap-2 rounded border border-destructive/30 bg-destructive/5 px-2.5 py-2 text-xs text-destructive">
          <AlertCircle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
          <span className="break-words">{job.error}</span>
        </div>
      )}

      {terminal && (job.status === "failed" || job.status === "interrupted") && (
        <div className="mt-3 flex justify-end border-t border-border/60 pt-3">
          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              void getTransport()
                .call("local_model_job_retry", { jobId: job.jobId })
                .catch((e) => toast.error(String(e)))
            }}
          >
            <RotateCcw className="mr-1.5 h-3.5 w-3.5" />
            {t("settings.knowledgeEmbedding.reembed.retry")}
          </Button>
        </div>
      )}
    </div>
  )
}

/**
 * Read bridge ③ — passive related-notes prompt (Phase 3, D7). When enabled, each
 * user turn surfaces the top accessible-KB note titles as an untrusted reference
 * block. Opt-in (off by default); access is already per-session gated so one
 * global toggle suffices. The enable switch saves immediately; the tuning knobs
 * use the three-state Save button.
 */
function PassiveRecallSection() {
  const { t } = useTranslation()
  const [loaded, setLoaded] = useState<PassiveRecallConfig | null>(null)
  const [draft, setDraft] = useState<PassiveRecallConfig | null>(null)
  const [saving, setSaving] = useState(false)
  const [saveStatus, setSaveStatus] = useState<"idle" | "saved" | "failed">("idle")

  useEffect(() => {
    let cancelled = false
    getTransport()
      .call<PassiveRecallConfig>("kb_passive_recall_config_get_cmd")
      .then((c) => {
        if (cancelled) return
        setLoaded(c)
        setDraft(c)
      })
      .catch((e) =>
        logger.error("settings", "PassiveRecallSection::load", "Failed to load config", e),
      )
    return () => {
      cancelled = true
    }
  }, [])

  const persist = useCallback(
    async (next: PassiveRecallConfig, viaButton: boolean) => {
      if (viaButton) setSaving(true)
      try {
        const saved = await getTransport().call<PassiveRecallConfig>(
          "kb_passive_recall_config_set_cmd",
          { config: next },
        )
        setLoaded(saved)
        if (viaButton) {
          // Save button: the draft IS what we persisted, so sync fully.
          setDraft(saved)
          setSaving(false)
          setSaveStatus("saved")
          setTimeout(() => setSaveStatus("idle"), 2000)
          toast.success(t("settings.knowledgePassiveRecall.saved", "Saved"))
        } else {
          // Enable toggle: persisted the last-saved knobs + new `enabled`, so keep
          // the user's in-progress knob edits — only reconcile `enabled`.
          setDraft((d) => (d ? { ...d, enabled: saved.enabled } : saved))
        }
      } catch (e) {
        logger.error("settings", "PassiveRecallSection::save", "Failed to save config", e)
        if (viaButton) {
          setSaving(false)
          setSaveStatus("failed")
          setTimeout(() => setSaveStatus("idle"), 2000)
        } else {
          // Revert the optimistic enable flip.
          setDraft((d) => (d ? { ...d, enabled: !next.enabled } : d))
        }
        toast.error(String(e))
      }
    },
    [t],
  )

  if (!draft || !loaded) return null

  const enabled = draft.enabled
  const dirty =
    loaded.topN !== draft.topN ||
    loaded.maxChars !== draft.maxChars ||
    loaded.cacheTtlSecs !== draft.cacheTtlSecs ||
    loaded.showSnippet !== draft.showSnippet

  return (
    <div className="rounded-lg border border-border bg-card">
      <div className="flex items-center justify-between px-4 py-3">
        <div className="min-w-0">
          <div className="text-sm font-medium">
            {t("settings.knowledgePassiveRecall.title", "Passive related notes")}
          </div>
          <div className="mt-0.5 text-xs text-muted-foreground">
            {t(
              "settings.knowledgePassiveRecall.description",
              "Each turn, surface titles of accessible notes related to your message as a hint.",
            )}
          </div>
        </div>
        <Switch
          checked={enabled}
          onCheckedChange={(v) => {
            // Optimistically flip in the draft; persist the LAST-SAVED knobs + new
            // `enabled` so toggling the master switch never silently commits the
            // user's unsaved knob edits (those stay pending behind the Save button).
            setDraft({ ...draft, enabled: v })
            void persist({ ...loaded, enabled: v }, false)
          }}
        />
      </div>

      <AnimatedCollapse open={enabled}>
        <div className="space-y-3 border-t border-border px-4 py-3">
          <div className="grid grid-cols-3 gap-3">
            <label className="space-y-1">
              <span className="text-xs font-medium">
                {t("settings.knowledgePassiveRecall.topN", "Max notes")}
              </span>
              <Input
                type="number"
                min={1}
                max={20}
                value={String(draft.topN)}
                onChange={(e) =>
                  setDraft({ ...draft, topN: Number.parseInt(e.target.value, 10) || draft.topN })
                }
                className="h-8 text-xs"
              />
            </label>
            <label className="space-y-1">
              <span className="text-xs font-medium">
                {t("settings.knowledgePassiveRecall.maxChars", "Max chars")}
              </span>
              <Input
                type="number"
                min={100}
                max={4000}
                value={String(draft.maxChars)}
                onChange={(e) =>
                  setDraft({
                    ...draft,
                    maxChars: Number.parseInt(e.target.value, 10) || draft.maxChars,
                  })
                }
                className="h-8 text-xs"
              />
            </label>
            <label className="space-y-1">
              <span className="text-xs font-medium">
                {t("settings.knowledgePassiveRecall.cacheTtlSecs", "Cache (s)")}
              </span>
              <Input
                type="number"
                min={1}
                value={String(draft.cacheTtlSecs)}
                onChange={(e) =>
                  setDraft({
                    ...draft,
                    cacheTtlSecs: Number.parseInt(e.target.value, 10) || draft.cacheTtlSecs,
                  })
                }
                className="h-8 text-xs"
              />
            </label>
          </div>
          <div className="flex items-center justify-between">
            <div className="min-w-0 pr-3">
              <div className="text-xs font-medium">
                {t("settings.knowledgePassiveRecall.showSnippet", "Show snippet")}
              </div>
              <div className="mt-0.5 text-[11px] text-muted-foreground">
                {t(
                  "settings.knowledgePassiveRecall.showSnippetDesc",
                  "Include a one-line excerpt under each title (more tokens).",
                )}
              </div>
            </div>
            <Switch
              checked={draft.showSnippet}
              onCheckedChange={(v) => setDraft({ ...draft, showSnippet: v })}
            />
          </div>
          <div className="flex justify-end">
            <Button
              size="sm"
              disabled={!dirty || saving}
              onClick={() => void persist(draft, true)}
              className={cn(saveStatus === "failed" && "bg-destructive hover:bg-destructive/90")}
            >
              {saving ? (
                <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />
              ) : saveStatus === "saved" ? (
                <Check className="mr-1.5 h-3.5 w-3.5 text-emerald-300" />
              ) : null}
              {t("common.save", "Save")}
            </Button>
          </div>
        </div>
      </AnimatedCollapse>
    </div>
  )
}

/**
 * Collapsible advanced section for note chunking (D12). Two scalars — chunk size
 * and overlap (code points) — both clamped server-side. Saving triggers a full
 * reindex of every KB (re-chunk + re-embed), so it carries an explicit warning;
 * progress surfaces in the reembed card above + the Knowledge view task panel.
 * Applies to FTS even when vector search is off, so it's always shown.
 */
function ChunkAdvancedSection() {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const [loaded, setLoaded] = useState<ChunkConfig | null>(null)
  const [maxChars, setMaxChars] = useState("")
  const [overlapChars, setOverlapChars] = useState("")
  const [saving, setSaving] = useState(false)
  const [saveStatus, setSaveStatus] = useState<"idle" | "saved" | "failed">("idle")

  useEffect(() => {
    let cancelled = false
    getTransport()
      .call<ChunkConfig>("knowledge_chunk_get_cmd")
      .then((c) => {
        if (cancelled) return
        setLoaded(c)
        setMaxChars(String(c.maxChars))
        setOverlapChars(String(c.overlapChars))
      })
      .catch((e) =>
        logger.error("settings", "ChunkAdvancedSection::load", "Failed to load chunk config", e),
      )
    return () => {
      cancelled = true
    }
  }, [])

  const mc = Number.parseInt(maxChars, 10)
  const oc = Number.parseInt(overlapChars, 10)
  const valid = Number.isFinite(mc) && Number.isFinite(oc) && mc > 0 && oc >= 0
  const dirty =
    !!loaded && (String(loaded.maxChars) !== maxChars || String(loaded.overlapChars) !== overlapChars)

  const save = useCallback(async () => {
    if (!valid || saving) return
    setSaving(true)
    try {
      const c = await getTransport().call<ChunkConfig>("knowledge_chunk_set_cmd", {
        maxChars: mc,
        overlapChars: oc,
      })
      setLoaded(c)
      setMaxChars(String(c.maxChars))
      setOverlapChars(String(c.overlapChars))
      setSaving(false)
      setSaveStatus("saved")
      setTimeout(() => setSaveStatus("idle"), 2000)
      toast.success(t("settings.knowledgeChunk.saved", "Saved — rebuilding every space"))
    } catch (e) {
      logger.error("settings", "ChunkAdvancedSection::save", "Failed to save chunk config", e)
      setSaving(false)
      setSaveStatus("failed")
      setTimeout(() => setSaveStatus("idle"), 2000)
      toast.error(String(e))
    }
  }, [valid, saving, mc, oc, t])

  return (
    <div className="rounded-lg border border-border bg-card">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center justify-between px-4 py-3"
      >
        <span className="flex items-center gap-2 text-sm font-medium">
          <Settings2 className="h-4 w-4 text-muted-foreground" />
          {t("settings.knowledgeChunk.title", "Advanced · chunking")}
        </span>
        <ChevronDown className={cn("h-4 w-4 transition-transform", open && "rotate-180")} />
      </button>
      <AnimatedCollapse open={open}>
        <div className="space-y-3 border-t border-border px-4 py-3">
          <p className="text-xs text-muted-foreground">
            {t(
              "settings.knowledgeChunk.description",
              "How notes are split into retrieval units. Defaults suit most notes.",
            )}
          </p>
          <div className="grid grid-cols-2 gap-3">
            <label className="space-y-1">
              <span className="text-xs font-medium">
                {t("settings.knowledgeChunk.maxChars", "Chunk size (chars)")}
              </span>
              <Input
                type="number"
                min={200}
                max={8000}
                value={maxChars}
                onChange={(e) => setMaxChars(e.target.value)}
                className="h-8 text-xs"
              />
            </label>
            <label className="space-y-1">
              <span className="text-xs font-medium">
                {t("settings.knowledgeChunk.overlap", "Overlap (chars)")}
              </span>
              <Input
                type="number"
                min={0}
                value={overlapChars}
                onChange={(e) => setOverlapChars(e.target.value)}
                className="h-8 text-xs"
              />
            </label>
          </div>
          <p className="flex items-start gap-1.5 text-[11px] text-amber-600 dark:text-amber-400">
            <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
            {t(
              "settings.knowledgeChunk.rebuildWarning",
              "Saving rebuilds the index for every knowledge space.",
            )}
          </p>
          <div className="flex justify-end">
            <Button
              size="sm"
              disabled={!valid || !dirty || saving}
              onClick={() => void save()}
              className={cn(saveStatus === "failed" && "bg-destructive hover:bg-destructive/90")}
            >
              {saving ? (
                <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />
              ) : saveStatus === "saved" ? (
                <Check className="mr-1.5 h-3.5 w-3.5 text-emerald-300" />
              ) : null}
              {t("common.save", "Save")}
            </Button>
          </div>
        </div>
      </AnimatedCollapse>
    </div>
  )
}
