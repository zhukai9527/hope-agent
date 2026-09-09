import { useState, useEffect, useCallback } from "react"
import { getTransport } from "@/lib/transport-provider"
import { useTranslation } from "react-i18next"
import { cn } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import { DeferredNumberInput } from "@/components/ui/deferred-number-input"
import { logger } from "@/lib/logger"
import { Check, Loader2, Timer } from "lucide-react"

interface LlmNetworkTimeoutConfig {
  connectTimeoutSecs: number
  totalTimeoutSecs: number
  firstTokenTimeoutSecs: number
}

const DEFAULT_TIMEOUT: LlmNetworkTimeoutConfig = {
  connectTimeoutSecs: 30,
  totalTimeoutSecs: 600,
  firstTokenTimeoutSecs: 120,
}

export default function LlmNetworkTimeoutSection() {
  const { t } = useTranslation()

  const [timeout, setTimeoutCfg] = useState<LlmNetworkTimeoutConfig>(DEFAULT_TIMEOUT)
  const [saved, setSaved] = useState("")
  const [saving, setSaving] = useState(false)
  const [status, setStatus] = useState<"idle" | "saved" | "failed">("idle")
  const [validation, setValidation] = useState("")

  const dirty = JSON.stringify(timeout) !== saved

  useEffect(() => {
    let cancelled = false
    getTransport()
      .call<LlmNetworkTimeoutConfig>("get_llm_network_timeout_config")
      .then((cfg) => {
        if (cancelled) return
        setSaved(JSON.stringify(cfg))
        setTimeoutCfg(cfg)
      })
      .catch((e) => {
        logger.error("settings", "LlmNetworkTimeoutSection::load", "Failed to load LLM network timeout", e)
      })
    return () => {
      cancelled = true
    }
  }, [])

  const validate = useCallback((next: LlmNetworkTimeoutConfig): string => {
    if (next.connectTimeoutSecs <= 0 || next.totalTimeoutSecs <= 0 || next.firstTokenTimeoutSecs <= 0) {
      return t("settings.llmNetworkTimeout.positive")
    }
    if (next.connectTimeoutSecs > next.totalTimeoutSecs) {
      return t("settings.llmNetworkTimeout.connectVsTotal")
    }
    if (next.firstTokenTimeoutSecs > next.totalTimeoutSecs) {
      return t("settings.llmNetworkTimeout.firstTokenVsTotal")
    }
    return ""
  }, [t])

  const save = useCallback(async () => {
    const problem = validate(timeout)
    if (problem) {
      setValidation(problem)
      return
    }
    setSaving(true)
    setValidation("")
    try {
      await getTransport().call("save_llm_network_timeout_config", { config: timeout })
      setSaved(JSON.stringify(timeout))
      setStatus("saved")
      setTimeout(() => setStatus("idle"), 2000)
    } catch (e) {
      logger.error("settings", "LlmNetworkTimeoutSection::save", "Failed to save LLM network timeout", e)
      setStatus("failed")
      setTimeout(() => setStatus("idle"), 2000)
    } finally {
      setSaving(false)
    }
  }, [timeout, validate, t])

  const setField = (key: keyof LlmNetworkTimeoutConfig, value: number) => {
    const next = { ...timeout, [key]: value }
    setValidation(validate(next))
    setTimeoutCfg(next)
  }

  const rows: Array<{ key: keyof LlmNetworkTimeoutConfig; label: string; desc: string }> = [
    { key: "connectTimeoutSecs", label: t("settings.llmNetworkTimeout.connect"), desc: t("settings.llmNetworkTimeout.connectDesc") },
    { key: "totalTimeoutSecs", label: t("settings.llmNetworkTimeout.total"), desc: t("settings.llmNetworkTimeout.totalDesc") },
    { key: "firstTokenTimeoutSecs", label: t("settings.llmNetworkTimeout.firstToken"), desc: t("settings.llmNetworkTimeout.firstTokenDesc") },
  ]

  return (
    <div className="w-full pt-6">
      <h3 className="text-sm font-semibold text-foreground mb-1 flex items-center gap-2">
        <Timer className="h-4 w-4 text-muted-foreground" />
        {t("settings.llmNetworkTimeout.title")}
      </h3>
      <p className="text-xs text-muted-foreground mb-3">{t("settings.llmNetworkTimeout.desc")}</p>
      <div className="space-y-4">
        {rows.map((row) => (
          <div key={row.key} className="flex items-start justify-between gap-4">
            <div className="min-w-0">
              <div className="text-sm font-medium">{row.label}</div>
              <div className="text-xs text-muted-foreground">{row.desc}</div>
            </div>
            <div className="flex items-center gap-2 shrink-0">
              <DeferredNumberInput
                className="w-24 text-right"
                value={timeout[row.key]}
                min={1}
                max={86400}
                onValueCommit={(v) => setField(row.key, v)}
              />
              <span className="text-xs text-muted-foreground">{t("settings.unitSeconds")}</span>
            </div>
          </div>
        ))}

        {validation && (
          <div className="px-3 py-2 rounded-md text-xs bg-destructive/10 text-destructive">{validation}</div>
        )}

        <div className="flex items-center justify-end gap-2">
          <Button
            size="sm"
            onClick={save}
            disabled={(!dirty && status === "idle") || saving || validation !== ""}
            className={cn(
              status === "saved" && "bg-green-500/10 text-green-600 hover:bg-green-500/20",
              status === "failed" && "bg-destructive/10 text-destructive hover:bg-destructive/20",
            )}
          >
            {saving ? (
              <span className="flex items-center gap-1.5">
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                {t("common.saving")}
              </span>
            ) : status === "saved" ? (
              <span className="flex items-center gap-1.5">
                <Check className="h-3.5 w-3.5" />
                {t("common.saved")}
              </span>
            ) : status === "failed" ? (
              t("common.saveFailed")
            ) : (
              t("common.save")
            )}
          </Button>
        </div>
      </div>
    </div>
  )
}
