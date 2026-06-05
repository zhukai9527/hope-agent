import { useState, useRef, useCallback, useEffect } from "react"
import { useTranslation } from "react-i18next"
import { useClickOutside } from "@/hooks/useClickOutside"
import { IconTip } from "@/components/ui/tooltip"
import { cn } from "@/lib/utils"
import { Library, Loader2, Lock } from "lucide-react"
import { Switch } from "@/components/ui/switch"
import { getTransport } from "@/lib/transport-provider"
import { logger } from "@/lib/logger"
import type {
  KnowledgeBaseMeta,
  KbAttachment,
  KbAccess,
  KbDraftAttachment,
} from "@/types/knowledge"

interface Props {
  sessionId: string | null
  projectId?: string | null
  /** Incognito sessions get zero KB access (design D10) — disable the control. */
  disabled?: boolean
  /**
   * Draft mode (no `sessionId` yet): parent-held staged attaches. When wired,
   * the picker reads/writes these instead of calling the backend; ChatScreen
   * forwards them on the `chat` command's `kbAttachments` payload, which the
   * backend applies on the auto-create branch. Ignored when a session exists
   * (live mode wins, using attach_session_kb_cmd directly).
   */
  draftAttachments?: KbDraftAttachment[]
  onDraftAttachChange?: (next: KbDraftAttachment[]) => void
}

/**
 * Compact popover for attaching knowledge spaces to the current session
 * (design D10 "currently effective knowledge bases" surface). Placed in the
 * chat input toolbar alongside AwarenessToggle.
 *
 * Attaching grants the assistant access: user `[[note]]` injection and the
 * `note_*` tools both gate on `effective_kb_access`, which reads exactly these
 * attach rows. Without an attach the default is deny — so this is the bridge
 * that makes the knowledge space reachable from chat.
 */
export default function KnowledgePicker({
  sessionId,
  projectId,
  disabled = false,
  draftAttachments = [],
  onDraftAttachChange,
}: Props) {
  const { t } = useTranslation()
  // Draft mode iff no live session and the parent wired a draft handler.
  const draftMode = !sessionId && !!onDraftAttachChange
  const [open, setOpen] = useState(false)
  const [kbs, setKbs] = useState<KnowledgeBaseMeta[]>([])
  const [attachments, setAttachments] = useState<KbAttachment[]>([])
  const [loading, setLoading] = useState(false)
  const [busyId, setBusyId] = useState<string | null>(null)
  const ref = useRef<HTMLDivElement>(null)

  useClickOutside(
    ref,
    useCallback(() => setOpen(false), []),
  )

  const loadAttachments = useCallback(() => {
    if (!sessionId) {
      setAttachments([])
      return
    }
    getTransport()
      .call<KbAttachment[]>("list_session_kbs_cmd", {
        sessionId,
        projectId: projectId ?? undefined,
      })
      .then(setAttachments)
      .catch(() => setAttachments([]))
  }, [sessionId, projectId])

  // Keep the badge count fresh as the session / project changes, and react to
  // KB mutations made elsewhere (knowledge view, agent tools, vault watcher).
  useEffect(() => {
    loadAttachments()
  }, [loadAttachments])

  useEffect(() => {
    return getTransport().listen("knowledge:changed", () => loadAttachments())
  }, [loadAttachments])

  useEffect(() => {
    if (disabled && open) setOpen(false)
  }, [disabled, open])

  // Load all available (non-archived) spaces when the popover opens.
  useEffect(() => {
    if (!open) return
    setLoading(true)
    getTransport()
      .call<KnowledgeBaseMeta[]>("list_kbs_cmd", { includeArchived: false })
      .then(setKbs)
      .catch(() => setKbs([]))
      .finally(() => setLoading(false))
    loadAttachments()
  }, [open, loadAttachments])

  // Prune staged drafts whose KB no longer exists / was archived elsewhere, so
  // the badge count and the replayed attach list stay consistent with the rows
  // we can actually render. Guarded on a real diff to avoid a render loop.
  useEffect(() => {
    if (!draftMode || !open || kbs.length === 0) return
    const valid = draftAttachments.filter((a) => kbs.some((k) => k.id === a.kbId))
    if (valid.length !== draftAttachments.length) onDraftAttachChange?.(valid)
  }, [draftMode, open, kbs, draftAttachments, onDraftAttachChange])

  const attachedCount = draftMode ? draftAttachments.length : attachments.length
  const btnDisabled = disabled || (!sessionId && !draftMode)

  // Normalize both live and draft attaches to the same `{ access, via }` shape the
  // row render consumes. Draft rows are always session-scoped (`via: "session"`).
  const attachmentFor = (id: string): { access: KbAccess; via: string } | undefined => {
    if (draftMode) {
      const d = draftAttachments.find((a) => a.kbId === id)
      return d ? { access: d.access, via: "session" } : undefined
    }
    return attachments.find((a) => a.id === id)
  }

  async function setAttach(kb: KnowledgeBaseMeta, access: KbAccess | null) {
    // Draft mode: mutate the parent-held list optimistically, no transport.
    if (draftMode) {
      const rest = draftAttachments.filter((a) => a.kbId !== kb.id)
      onDraftAttachChange!(access === null ? rest : [...rest, { kbId: kb.id, access }])
      return
    }
    if (!sessionId) return
    setBusyId(kb.id)
    try {
      if (access === null) {
        await getTransport().call("detach_session_kb_cmd", { sessionId, kbId: kb.id })
      } else {
        await getTransport().call("attach_session_kb_cmd", { sessionId, kbId: kb.id, access })
      }
      loadAttachments()
    } catch (e) {
      logger.error("chat", "KnowledgePicker::setAttach", "attach/detach failed", e)
    } finally {
      setBusyId(null)
    }
  }

  const tipLabel = disabled
    ? t("knowledge.picker.incognitoDisabled")
    : !sessionId && !draftMode
      ? t("knowledge.picker.needSession")
      : t("knowledge.picker.title")

  return (
    <div className="relative" ref={ref}>
      <IconTip label={tipLabel}>
        <button
          type="button"
          disabled={btnDisabled}
          onClick={() => setOpen(!open)}
          className={cn(
            "flex items-center gap-1 bg-transparent text-xs font-medium px-2 py-1 rounded-lg cursor-pointer transition-colors hover:bg-secondary shrink-0 disabled:cursor-not-allowed disabled:opacity-50",
            attachedCount > 0
              ? "text-blue-500"
              : "text-muted-foreground hover:text-foreground",
          )}
        >
          <Library className="h-4 w-4" />
          {attachedCount > 0 && <span className="tabular-nums">{attachedCount}</span>}
        </button>
      </IconTip>

      {open && !btnDisabled && (
        <div className="absolute bottom-full left-0 mb-2 w-[320px] bg-popover/95 backdrop-blur-xl border border-border/60 rounded-xl shadow-[0_8px_30px_rgb(0,0,0,0.12)] z-50 p-3 animate-in fade-in-0 zoom-in-95 slide-in-from-bottom-1 duration-150">
          <div className="flex items-center justify-between mb-2">
            <span className="text-[11px] text-muted-foreground font-medium">
              {t("knowledge.picker.heading")}
            </span>
            {loading && <Loader2 className="h-3 w-3 animate-spin text-muted-foreground" />}
          </div>

          {kbs.length === 0 && !loading ? (
            <p className="text-xs text-muted-foreground py-3 text-center">
              {t("knowledge.picker.empty")}
            </p>
          ) : (
            <div className="flex flex-col gap-0.5 max-h-[280px] overflow-y-auto -mx-1 px-1">
              {kbs.map((kb) => {
                const att = attachmentFor(kb.id)
                const viaProject = att?.via === "project"
                const busy = busyId === kb.id
                return (
                  <div
                    key={kb.id}
                    className="flex items-center gap-2 rounded-lg px-1.5 py-1.5 hover:bg-secondary/50"
                  >
                    <span className="shrink-0 text-sm leading-none">{kb.emoji || "📚"}</span>
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-1">
                        <span className="truncate text-xs font-medium">{kb.name}</span>
                        {kb.external && (
                          <Lock className="h-3 w-3 shrink-0 text-muted-foreground" />
                        )}
                      </div>
                      <span className="text-[10px] text-muted-foreground">
                        {viaProject
                          ? t("knowledge.picker.viaProject")
                          : t("knowledge.picker.noteCount", { count: kb.noteCount })}
                      </span>
                    </div>

                    {/* Access toggle — only for session-scoped, internal, attached spaces.
                        External vaults are capped to read (D11); project-scoped attaches
                        are managed at the project level. */}
                    {att && !viaProject && !kb.external && (
                      <button
                        type="button"
                        disabled={busy}
                        onClick={() =>
                          setAttach(kb, att.access === "read" ? "write" : "read")
                        }
                        className="shrink-0 whitespace-nowrap rounded-md border border-border/60 px-1.5 py-0.5 text-[10px] text-muted-foreground transition-colors hover:bg-secondary hover:text-foreground disabled:opacity-50"
                      >
                        {att.access === "write"
                          ? t("knowledge.picker.write")
                          : t("knowledge.picker.read")}
                      </button>
                    )}
                    {att && (viaProject || kb.external) && (
                      <span className="shrink-0 rounded-md bg-secondary/60 px-1.5 py-0.5 text-[10px] text-muted-foreground">
                        {t("knowledge.picker.read")}
                      </span>
                    )}

                    <Switch
                      checked={!!att}
                      disabled={busy || viaProject}
                      onCheckedChange={(v) => setAttach(kb, v ? "read" : null)}
                    />
                  </div>
                )
              })}
            </div>
          )}

          <p className="mt-2 text-[10px] text-muted-foreground leading-relaxed">
            {t("knowledge.picker.hint")}
          </p>
        </div>
      )}
    </div>
  )
}
