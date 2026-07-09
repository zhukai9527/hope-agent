import { useState, useRef, useEffect, useCallback } from "react"
import { useTranslation } from "react-i18next"
import { cn } from "@/lib/utils"
import { Check, ChevronDown, ChevronRight } from "lucide-react"
import { Slider } from "@/components/ui/slider"
import { FloatingMenu } from "@/components/ui/floating-menu"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import type { AvailableModel, ActiveModel } from "@/types/chat"
import { getEffortOptionsForModel, modelSupportsThinking } from "@/types/chat"

const ROOT_MENU_WIDTH = 260
const MODEL_SUBMENU_WIDTH = 280
const TEMPERATURE_SUBMENU_WIDTH = 220
const SUBMENU_GAP = 6
const VIEWPORT_MARGIN = 8

interface ModelPickerProps {
  availableModels: AvailableModel[]
  activeModel: ActiveModel | null
  reasoningEffort: string
  onModelChange: (key: string) => void
  onEffortChange: (effort: string) => void
  currentModelInfo?: AvailableModel
  sessionTemperature?: number | null
  onSessionTemperatureChange?: (temp: number | null) => void
}

export default function ModelPicker({
  availableModels,
  activeModel,
  reasoningEffort,
  onModelChange,
  onEffortChange,
  currentModelInfo,
  sessionTemperature,
  onSessionTemperatureChange,
}: ModelPickerProps) {
  const { t } = useTranslation()
  const [showMenu, setShowMenu] = useState(false)
  const [openPanel, setOpenPanel] = useState<"model" | "temperature" | null>(null)
  const [submenuPlacement, setSubmenuPlacement] = useState<"right" | "top">("right")
  const menuRef = useRef<HTMLDivElement>(null)

  const placeSubmenu = useCallback((panel: "model" | "temperature") => {
    const root = menuRef.current
    if (!root) {
      setSubmenuPlacement("right")
      return
    }

    const submenuWidth = panel === "model" ? MODEL_SUBMENU_WIDTH : TEMPERATURE_SUBMENU_WIDTH
    const rootLeft = root.getBoundingClientRect().left
    const sideMenuRight = rootLeft + ROOT_MENU_WIDTH + SUBMENU_GAP + submenuWidth
    setSubmenuPlacement(sideMenuRight > window.innerWidth - VIEWPORT_MARGIN ? "top" : "right")
  }, [])

  const setNestedPanel = useCallback((panel: "model" | "temperature" | null) => {
    if (panel) placeSubmenu(panel)
    setOpenPanel(panel)
  }, [placeSubmenu])

  useEffect(() => {
    function handleClickOutside(e: MouseEvent) {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setShowMenu(false)
        setOpenPanel(null)
      }
    }
    if (showMenu) {
      document.addEventListener("mousedown", handleClickOutside)
      return () => document.removeEventListener("mousedown", handleClickOutside)
    }
  }, [showMenu])

  useEffect(() => {
    if (!showMenu || !openPanel) return
    const updatePlacement = () => placeSubmenu(openPanel)
    updatePlacement()
    window.addEventListener("resize", updatePlacement)
    return () => window.removeEventListener("resize", updatePlacement)
  }, [openPanel, placeSubmenu, showMenu])

  const supportsThinking = modelSupportsThinking(currentModelInfo)
  const effortOptions = getEffortOptionsForModel(currentModelInfo, t)
  const effortLabel =
    effortOptions.find((o) => o.value === reasoningEffort)?.label ?? reasoningEffort
  const modelLabel = currentModelInfo?.modelName ?? t("chat.selectModel")
  const temperatureLabel =
    sessionTemperature != null ? sessionTemperature.toFixed(2) : t("settings.temperatureDefault")

  const modelGroups = Array.from(
    availableModels.reduce((groups, model) => {
      const existing = groups.get(model.providerId)
      if (existing) {
        existing.models.push(model)
      } else {
        groups.set(model.providerId, {
          providerName: model.providerName,
          models: [model],
        })
      }
      return groups
    }, new Map<string, { providerName: string; models: AvailableModel[] }>()),
  )

  return (
    <div className="relative shrink-0" ref={menuRef}>
      <Tooltip>
        <TooltipTrigger asChild>
          <button
            type="button"
            onClick={() => {
              setShowMenu(!showMenu)
              setOpenPanel(null)
            }}
            className="flex max-w-[220px] items-center gap-1 overflow-hidden bg-transparent px-2 py-1 text-xs font-medium text-muted-foreground transition-colors hover:bg-secondary hover:text-foreground rounded-lg cursor-pointer"
          >
            <span className="min-w-0 truncate">{modelLabel}</span>
            {supportsThinking && (
              <span
                className={cn(
                  "shrink-0 whitespace-nowrap",
                  reasoningEffort !== "none" && "text-foreground/80",
                )}
              >
                {effortLabel}
              </span>
            )}
            <ChevronDown className="h-3.5 w-3.5 shrink-0 opacity-60" />
          </button>
        </TooltipTrigger>
        {/* Full, untruncated model name on hover — the trigger caps at
            max-w-[220px] and truncates, so this is the only way to read a
            long name without opening the menu. */}
        <TooltipContent side="top">
          {currentModelInfo?.providerName
            ? `${currentModelInfo.providerName} · ${modelLabel}`
            : modelLabel}
          {supportsThinking && reasoningEffort !== "none" ? ` · ${effortLabel}` : ""}
        </TooltipContent>
      </Tooltip>

      <FloatingMenu
        open={showMenu}
        className="w-[260px] overflow-visible p-1.5"
        onEscapeKeyDown={() => {
          setShowMenu(false)
          setNestedPanel(null)
        }}
      >
        <div className="max-h-[min(420px,calc(100vh-96px))] overflow-y-auto overscroll-contain pr-0.5">
          <div className="px-2.5 pb-1 pt-1 text-[11px] font-semibold text-muted-foreground">
            {t("settings.localModels.filters.thinking")}
          </div>
          {supportsThinking ? (
            <div className="flex flex-col gap-0.5">
              {effortOptions.map((opt) => (
                <button
                  key={opt.value}
                  type="button"
                  className={cn(
                    "flex w-full items-center justify-between gap-3 rounded-md px-2.5 py-1.5 text-left text-[13px] transition-all duration-150",
                    reasoningEffort === opt.value
                      ? "bg-secondary text-foreground font-medium shadow-sm"
                      : "text-foreground/80 hover:bg-secondary/60 hover:text-foreground",
                  )}
                  onMouseEnter={() => setNestedPanel(null)}
                  onClick={() => {
                    onEffortChange(opt.value)
                    setShowMenu(false)
                    setNestedPanel(null)
                  }}
                >
                  <span className="truncate">{opt.label}</span>
                  {reasoningEffort === opt.value && (
                    <Check className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                  )}
                </button>
              ))}
            </div>
          ) : (
            <p className="px-2.5 pb-1 pt-0.5 text-[11px] leading-relaxed text-muted-foreground/70">
              {t("chat.reasoningDisabledHint")}
            </p>
          )}

          <div className="-mx-1 my-1.5 h-px bg-border-soft" />

          {availableModels.length > 0 && (
            <button
              type="button"
              className={cn(
                "flex w-full items-center justify-between gap-3 rounded-md px-2.5 py-1.5 text-left text-[13px] transition-all duration-150",
                openPanel === "model"
                  ? "bg-secondary text-foreground shadow-sm"
                  : "text-foreground/80 hover:bg-secondary/60 hover:text-foreground",
              )}
              onMouseEnter={() => setNestedPanel("model")}
              onClick={() => setNestedPanel(openPanel === "model" ? null : "model")}
            >
              <span className="truncate">{modelLabel}</span>
              <ChevronRight className="h-3.5 w-3.5 shrink-0 opacity-50" />
            </button>
          )}

          <button
            type="button"
            className={cn(
              "flex w-full items-center justify-between gap-3 rounded-md px-2.5 py-1.5 text-left text-[13px] transition-all duration-150",
              openPanel === "temperature"
                ? "bg-secondary text-foreground shadow-sm"
                : "text-foreground/80 hover:bg-secondary/60 hover:text-foreground",
            )}
            onMouseEnter={() => setNestedPanel("temperature")}
            onClick={() => setNestedPanel(openPanel === "temperature" ? null : "temperature")}
          >
            <span className="truncate">{t("settings.temperature")}</span>
            <span className="ml-auto shrink-0 text-xs text-muted-foreground tabular-nums">
              {temperatureLabel}
            </span>
            <ChevronRight className="h-3.5 w-3.5 shrink-0 opacity-50" />
          </button>
        </div>

        <FloatingMenu
          open={openPanel === "model"}
          positionClassName={
            submenuPlacement === "top" ? "bottom-full left-0 mb-1.5" : "bottom-0 left-full ml-1.5"
          }
          originClassName={submenuPlacement === "top" ? "origin-bottom-left" : "origin-left"}
          className={cn(
            submenuPlacement === "top" ? "ha-menu-from-top" : "ha-menu-from-left",
            "w-[280px] p-1.5",
          )}
        >
          <div className="max-h-[min(360px,calc(100vh-112px))] overflow-y-auto overscroll-contain">
            {modelGroups.map(([providerId, group]) => (
              <div key={providerId} className="py-0.5">
                {modelGroups.length > 1 && (
                  <div className="px-2.5 py-1 text-[10px] font-semibold uppercase tracking-wide text-muted-foreground/70">
                    {group.providerName}
                  </div>
                )}
                <div className="flex flex-col gap-0.5">
                  {group.models.map((model) => {
                    const selected =
                      activeModel?.providerId === model.providerId &&
                      activeModel?.modelId === model.modelId
                    return (
                      <button
                        key={`${model.providerId}::${model.modelId}`}
                        type="button"
                        className={cn(
                          "flex w-full items-center justify-between gap-3 rounded-md px-2.5 py-1.5 text-left text-[13px] transition-all duration-150",
                          selected
                            ? "bg-secondary text-foreground font-medium shadow-sm"
                            : "text-foreground/80 hover:bg-secondary/60 hover:text-foreground",
                        )}
                        onClick={() => {
                          onModelChange(`${model.providerId}::${model.modelId}`)
                          setShowMenu(false)
                          setNestedPanel(null)
                        }}
                      >
                        <span className="truncate">{model.modelName}</span>
                        {selected && (
                          <Check className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                        )}
                      </button>
                    )
                  })}
                </div>
              </div>
            ))}
          </div>
        </FloatingMenu>

        <FloatingMenu
          open={openPanel === "temperature"}
          positionClassName={
            submenuPlacement === "top" ? "bottom-full left-0 mb-1.5" : "bottom-0 left-full ml-1.5"
          }
          originClassName={submenuPlacement === "top" ? "origin-bottom-left" : "origin-left"}
          className={cn(
            submenuPlacement === "top" ? "ha-menu-from-top" : "ha-menu-from-left",
            "w-[220px] p-3",
          )}
        >
          <div className="mb-2 flex items-center justify-between gap-3">
            <span className="text-[11px] font-medium text-muted-foreground">
              {t("settings.temperature")}
            </span>
            <span className="text-xs font-mono text-foreground tabular-nums">
              {temperatureLabel}
            </span>
          </div>
          <Slider
            min={0}
            max={200}
            step={1}
            value={[sessionTemperature != null ? Math.round(sessionTemperature * 100) : 100]}
            onValueChange={([v]) => {
              onSessionTemperatureChange?.(v / 100)
            }}
            disabled={!onSessionTemperatureChange}
          />
          <div className="mt-2 flex items-center justify-between gap-3">
            <span className="text-[10px] text-muted-foreground/60">
              {t("settings.temperaturePrecise")}
            </span>
            <button
              type="button"
              className="text-[10px] text-primary transition-colors hover:text-primary/80 disabled:pointer-events-none disabled:opacity-50"
              disabled={!onSessionTemperatureChange}
              onClick={() => {
                onSessionTemperatureChange?.(null)
                setShowMenu(false)
                setNestedPanel(null)
              }}
            >
              {t("settings.temperatureReset")}
            </button>
          </div>
        </FloatingMenu>
      </FloatingMenu>
    </div>
  )
}
