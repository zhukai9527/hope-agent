import { useEffect, useRef, useState } from "react"
import { ChevronLeft, ChevronRight } from "lucide-react"
import { useTranslation } from "react-i18next"

import { Button } from "@/components/ui/button"
import { IconTip } from "@/components/ui/tooltip"
import { OfficeLoading } from "./OfficeLoading"
import type { OfficeViewProps } from "./types"

/**
 * Renders a `.pptx` slide-by-slide onto a `<canvas>` via `pptxviewjs`
 * (lazy-loaded). Canvas output means text isn't selectable and animations /
 * transitions aren't reproduced — the inherent limit of client-side pptx
 * rendering. Failures bubble through `onError` to the text fallback.
 */
export function PptxView({ data, onError }: OfficeViewProps) {
  const { t } = useTranslation()
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const viewerRef = useRef<import("pptxviewjs").PPTXViewer | null>(null)
  const [loading, setLoading] = useState(true)
  const [count, setCount] = useState(0)
  const [current, setCurrent] = useState(0)
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    void (async () => {
      try {
        const { PPTXViewer } = await import("pptxviewjs")
        if (cancelled || !canvasRef.current) return
        const viewer = new PPTXViewer({ canvas: canvasRef.current, slideSizeMode: "fit" })
        await viewer.loadFile(data)
        if (cancelled) {
          viewer.destroy()
          return
        }
        await viewer.render()
        if (cancelled) {
          viewer.destroy()
          return
        }
        viewerRef.current = viewer
        setCount(viewer.getSlideCount())
        setCurrent(viewer.getCurrentSlideIndex())
        setLoading(false)
      } catch (e) {
        if (!cancelled) onError(e)
      }
    })()
    return () => {
      cancelled = true
      try {
        viewerRef.current?.destroy()
      } catch {
        /* ignore teardown errors */
      }
      viewerRef.current = null
    }
  }, [data, onError])

  const go = async (dir: -1 | 1) => {
    const v = viewerRef.current
    if (!v || busy) return
    setBusy(true)
    try {
      await (dir === 1 ? v.nextSlide() : v.previousSlide())
      setCurrent(v.getCurrentSlideIndex())
    } catch (e) {
      onError(e)
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="flex h-full flex-col">
      <div className="relative flex-1 overflow-auto bg-muted/30 p-3">
        {loading && (
          <div className="absolute inset-0 z-10 flex items-start justify-center bg-background/60">
            <OfficeLoading />
          </div>
        )}
        <canvas ref={canvasRef} className="mx-auto h-auto max-w-full" />
      </div>
      {count > 0 && (
        <div className="flex shrink-0 items-center justify-center gap-3 border-t border-border px-3 py-2">
          <IconTip label={t("fileBrowser.prevSlide", "Previous slide")}>
            <Button
              variant="ghost"
              size="icon"
              className="h-7 w-7"
              disabled={busy || current <= 0}
              onClick={() => void go(-1)}
            >
              <ChevronLeft className="h-4 w-4" />
            </Button>
          </IconTip>
          <span className="text-xs tabular-nums text-muted-foreground">
            {current + 1} / {count}
          </span>
          <IconTip label={t("fileBrowser.nextSlide", "Next slide")}>
            <Button
              variant="ghost"
              size="icon"
              className="h-7 w-7"
              disabled={busy || current >= count - 1}
              onClick={() => void go(1)}
            >
              <ChevronRight className="h-4 w-4" />
            </Button>
          </IconTip>
        </div>
      )}
    </div>
  )
}
