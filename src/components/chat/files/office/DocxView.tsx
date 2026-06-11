import { useEffect, useRef, useState } from "react"

import { OfficeLoading } from "./OfficeLoading"
import type { OfficeViewProps } from "./types"

/**
 * Renders a `.docx` into near-original HTML via `docx-preview` (lazy-loaded).
 * `renderAsync` injects its `<style>` into a dedicated hidden `styleRef` (not
 * `document.head`) and prefixes every class with `docx`, keeping the document's
 * CSS scoped to this preview instead of leaking into the app.
 */
export function DocxView({ data, onError }: OfficeViewProps) {
  const bodyRef = useRef<HTMLDivElement>(null)
  const styleRef = useRef<HTMLDivElement>(null)
  const [rendering, setRendering] = useState(true)

  useEffect(() => {
    let cancelled = false
    const body = bodyRef.current
    const style = styleRef.current
    if (!body || !style) return
    setRendering(true)
    void (async () => {
      try {
        const { renderAsync } = await import("docx-preview")
        if (cancelled) return
        body.replaceChildren()
        style.replaceChildren()
        await renderAsync(data, body, style, {
          className: "docx",
          inWrapper: true,
          breakPages: true,
        })
        if (!cancelled) setRendering(false)
      } catch (e) {
        if (!cancelled) onError(e)
      }
    })()
    return () => {
      cancelled = true
      body.replaceChildren()
      style.replaceChildren()
    }
  }, [data, onError])

  return (
    <div className="relative h-full overflow-auto bg-muted/30">
      {rendering && (
        <div className="absolute inset-0 z-10 flex items-start justify-center bg-background/60">
          <OfficeLoading />
        </div>
      )}
      <div ref={styleRef} className="hidden" aria-hidden="true" />
      <div ref={bodyRef} />
    </div>
  )
}
