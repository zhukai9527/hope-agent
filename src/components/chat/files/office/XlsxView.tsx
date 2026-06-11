import { useEffect, useState } from "react"

import { OfficeLoading } from "./OfficeLoading"
import type { OfficeViewProps } from "./types"

interface SheetHtml {
  name: string
  html: string
}

/**
 * Renders an `.xlsx` / `.xls` as HTML tables via SheetJS (lazy-loaded).
 * `sheet_to_html` preserves merged cells (colspan/rowspan) and escapes cell
 * text; table chrome (borders, header row, horizontal scroll) is applied with
 * Tailwind arbitrary-descendant utilities on the wrapper. Cell fill/font colors
 * are not preserved (SheetJS community edition limitation).
 */
export function XlsxView({ data, onError }: OfficeViewProps) {
  const [sheets, setSheets] = useState<SheetHtml[] | null>(null)

  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const XLSX = await import("xlsx")
        if (cancelled) return
        const wb = XLSX.read(new Uint8Array(data), { type: "array" })
        const out: SheetHtml[] = []
        for (const name of wb.SheetNames) {
          const ws = wb.Sheets[name]
          if (ws) out.push({ name, html: XLSX.utils.sheet_to_html(ws, { editable: false }) })
        }
        if (!cancelled) setSheets(out)
      } catch (e) {
        if (!cancelled) onError(e)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [data, onError])

  if (!sheets) return <OfficeLoading />

  return (
    <div className="h-full space-y-6 overflow-auto p-3">
      {sheets.map((s) => (
        <section key={s.name} className="space-y-2">
          {sheets.length > 1 && <h3 className="text-sm font-semibold text-foreground">{s.name}</h3>}
          <div
            className="overflow-x-auto text-sm [&_table]:border-collapse [&_td]:whitespace-nowrap [&_td]:border [&_td]:border-border [&_td]:px-2 [&_td]:py-1 [&_td]:align-top [&_tr:first-child]:bg-muted [&_tr:first-child]:font-medium"
            // SheetJS escapes cell text; the file is the user's own local document.
            dangerouslySetInnerHTML={{ __html: s.html }}
          />
        </section>
      ))}
    </div>
  )
}
