import { useCallback, useEffect, useState } from "react"
import { useTranslation } from "react-i18next"
import { getTransport } from "@/lib/transport-provider"
import { logger } from "@/lib/logger"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Loader2 } from "lucide-react"

// Mirrors ha-core `ClaimRecord` (camelCase).
interface ClaimRecord {
  id: string
  scopeType: string
  scopeId?: string | null
  claimType: string
  subject: string
  predicate: string
  object: string
  content: string
  tags: string[]
  confidence: number
  confidenceSource: string
  salience: number
  status: string
  validUntil?: string | null
  createdAt: string
  updatedAt: string
}

interface EvidenceRecord {
  id: string
  sourceType: string
  evidenceClass: string
  sourceId: string
  sessionId?: string | null
  messageId?: string | null
  quote?: string | null
  createdAt: string
}

interface ClaimLink {
  claimId: string
  memoryId: number
  syncMode: string
}

interface ClaimDetail {
  claim: ClaimRecord
  evidence: EvidenceRecord[]
  links: ClaimLink[]
}

const STATUS_DOT: Record<string, string> = {
  active: "bg-emerald-500",
  superseded: "bg-amber-500",
  expired: "bg-muted-foreground/50",
  archived: "bg-muted-foreground/50",
  needs_review: "bg-sky-500",
}

/**
 * Read-only "Claims (beta)" view over the next-gen structured memory. Lists
 * claims via `claim_list` and shows a selected claim's evidence + legacy-memory
 * links via `claim_get`. No editing — the review/correction UI lands later.
 */
export default function ClaimsBetaView() {
  const { t } = useTranslation()
  const [claims, setClaims] = useState<ClaimRecord[]>([])
  const [loading, setLoading] = useState(false)
  const [statusFilter, setStatusFilter] = useState<string>("all")
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [detail, setDetail] = useState<ClaimDetail | null>(null)

  const loadClaims = useCallback(async () => {
    setLoading(true)
    // Reset the selection so the detail pane can't show a claim the new
    // filter excludes (stale-detail guard).
    setSelectedId(null)
    setDetail(null)
    try {
      const args: Record<string, unknown> = { limit: 200 }
      if (statusFilter !== "all") args.status = statusFilter
      const list = await getTransport().call<ClaimRecord[]>("claim_list", args)
      setClaims(list ?? [])
    } catch (e) {
      logger.error("settings", "ClaimsBetaView::list", "Failed to list claims", e)
      setClaims([])
    } finally {
      setLoading(false)
    }
  }, [statusFilter])

  const loadDetail = useCallback(async (id: string) => {
    try {
      const d = await getTransport().call<ClaimDetail | null>("claim_get", { id })
      setDetail(d ?? null)
    } catch (e) {
      logger.error("settings", "ClaimsBetaView::get", "Failed to load claim", e)
      setDetail(null)
    }
  }, [])

  useEffect(() => {
    void loadClaims()
  }, [loadClaims])

  useEffect(() => {
    if (selectedId) void loadDetail(selectedId)
    else setDetail(null)
  }, [selectedId, loadDetail])

  const scopeLabel = (c: ClaimRecord) =>
    c.scopeType === "global" ? "global" : `${c.scopeType}:${c.scopeId ?? "?"}`

  return (
    <div className="flex-1 overflow-y-auto p-6 space-y-3">
      <div className="flex items-center justify-between gap-2">
        <div>
          <div className="text-sm font-medium flex items-center gap-1.5">
            {t("settings.claims.title")}
            <span className="text-[9px] uppercase tracking-wide rounded bg-primary/15 text-primary px-1 py-0.5">
              beta
            </span>
          </div>
          <div className="text-xs text-muted-foreground">{t("settings.claims.desc")}</div>
        </div>
        <Select value={statusFilter} onValueChange={setStatusFilter}>
          <SelectTrigger className="h-8 w-[140px] text-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">{t("settings.claims.statusAll")}</SelectItem>
            <SelectItem value="active">{t("settings.claims.status.active")}</SelectItem>
            <SelectItem value="superseded">{t("settings.claims.status.superseded")}</SelectItem>
            <SelectItem value="expired">{t("settings.claims.status.expired")}</SelectItem>
            <SelectItem value="archived">{t("settings.claims.status.archived")}</SelectItem>
            <SelectItem value="needs_review">{t("settings.claims.status.needs_review")}</SelectItem>
          </SelectContent>
        </Select>
      </div>

      <div className="grid grid-cols-[1fr_1fr] gap-4">
        {/* Claim list */}
        <div className="border border-border/60 rounded-lg overflow-hidden">
          <div className="px-3 py-2 border-b border-border/60 bg-secondary/20 text-xs font-medium">
            {t("settings.claims.list")} ({claims.length})
          </div>
          <div className="max-h-[460px] overflow-y-auto">
            {loading ? (
              <div className="px-3 py-6 text-xs text-muted-foreground text-center inline-flex items-center gap-1 w-full justify-center">
                <Loader2 className="h-3 w-3 animate-spin" />
                {t("common.loading")}
              </div>
            ) : claims.length === 0 ? (
              <div className="px-3 py-6 text-xs text-muted-foreground text-center">
                {t("settings.claims.empty")}
              </div>
            ) : (
              claims.map((c) => (
                <button
                  key={c.id}
                  onClick={() => setSelectedId(c.id)}
                  className={`w-full text-left px-3 py-2 text-xs hover:bg-secondary/40 transition-colors border-b border-border/30 ${
                    selectedId === c.id ? "bg-secondary/60 font-medium" : ""
                  }`}
                >
                  <div className="flex items-center gap-2">
                    <span
                      className={`h-2 w-2 rounded-full shrink-0 ${STATUS_DOT[c.status] ?? "bg-muted-foreground/50"}`}
                    />
                    <span className="truncate">{c.content}</span>
                  </div>
                  <div className="text-[10px] text-muted-foreground mt-0.5 font-mono">
                    {c.claimType} · {scopeLabel(c)} · {(c.confidence * 100).toFixed(0)}%
                  </div>
                </button>
              ))
            )}
          </div>
        </div>

        {/* Claim detail */}
        <div className="border border-border/60 rounded-lg p-3 max-h-[460px] overflow-y-auto">
          {detail ? (
            <div className="text-xs space-y-2">
              <div className="font-medium">{detail.claim.content}</div>
              <div className="font-mono text-[10px] text-muted-foreground">
                {detail.claim.subject} · {detail.claim.predicate} · {detail.claim.object}
              </div>
              <div className="text-[10px] text-muted-foreground">
                {t("settings.claims.confidence")}: {(detail.claim.confidence * 100).toFixed(0)}% (
                {detail.claim.confidenceSource}) · {t("settings.claims.salience")}:{" "}
                {(detail.claim.salience * 100).toFixed(0)}%
                {detail.claim.validUntil ? ` · until ${detail.claim.validUntil}` : ""}
              </div>
              {detail.claim.tags.length > 0 && (
                <div className="flex flex-wrap gap-1">
                  {detail.claim.tags.map((tag) => (
                    <span
                      key={tag}
                      className="rounded bg-secondary/60 px-1.5 py-0.5 text-[10px] text-muted-foreground"
                    >
                      {tag}
                    </span>
                  ))}
                </div>
              )}

              <div className="font-medium pt-1">
                {t("settings.claims.evidence")} ({detail.evidence.length})
              </div>
              {detail.evidence.length === 0 ? (
                <div className="text-muted-foreground">{t("settings.claims.noEvidence")}</div>
              ) : (
                <ul className="space-y-1">
                  {detail.evidence.map((e) => (
                    <li
                      key={e.id}
                      className="rounded border border-border/40 px-2 py-1 font-mono text-[10px] text-muted-foreground"
                    >
                      {e.sourceType} · {e.evidenceClass}
                      {e.sessionId ? ` · session ${e.sessionId.slice(0, 8)}…` : ""}
                      {e.messageId ? ` #${e.messageId}` : ""}
                    </li>
                  ))}
                </ul>
              )}

              {detail.links.length > 0 && (
                <>
                  <div className="font-medium pt-1">
                    {t("settings.claims.links")} ({detail.links.length})
                  </div>
                  <ul className="space-y-1">
                    {detail.links.map((l) => (
                      <li
                        key={`${l.claimId}-${l.memoryId}`}
                        className="font-mono text-[10px] text-muted-foreground"
                      >
                        memory #{l.memoryId} · {l.syncMode}
                      </li>
                    ))}
                  </ul>
                </>
              )}
            </div>
          ) : (
            <div className="text-xs text-muted-foreground text-center py-12">
              {t("settings.claims.selectClaim")}
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
