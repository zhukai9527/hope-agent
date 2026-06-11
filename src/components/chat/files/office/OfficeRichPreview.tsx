import { useCallback, useEffect, useState } from "react"

import type { PreviewSource } from "../previewSource"
import { DocxView } from "./DocxView"
import { officeFormatOf } from "./officeFormat"
import { OfficeLoading } from "./OfficeLoading"
import { OfficeTextFallback } from "./OfficeTextFallback"
import { PptxView } from "./PptxView"
import { XlsxView } from "./XlsxView"

/** Above this, skip rich rendering (parse/DOM cost) and fall back to text. */
const MAX_RICH_BYTES = 30 * 1024 * 1024

/**
 * Rich preview for office files: resolve the sub-format, fetch the raw bytes via
 * the source's authorized `rawUrl` (Tauri asset / HTTP by-path — the same
 * channel image/pdf previews use), and hand them to the matching lazy-loaded
 * renderer (docx-preview / SheetJS / pptxviewjs). Anything that can't render
 * richly — unsupported format, oversized file, fetch/lib failure — flips to
 * {@link OfficeTextFallback} (the backend's plain-text extraction), so this is
 * never worse than the previous text-only preview.
 *
 * Mount with a `key` tied to the file (the caller does) so a new file resets
 * state via remount — the effect only fetches, it never resets synchronously.
 */
export function OfficeRichPreview({ source }: { source: PreviewSource }) {
  const format = officeFormatOf(source.name, source.mime)
  const tooBig = source.sizeBytes != null && source.sizeBytes > MAX_RICH_BYTES
  const eligible = format !== null && !tooBig

  const [data, setData] = useState<ArrayBuffer | null>(null)
  const [failed, setFailed] = useState(false)

  useEffect(() => {
    if (!eligible) return
    let cancelled = false
    void (async () => {
      try {
        const url = await source.rawUrl(false)
        if (cancelled) return
        if (!url) {
          setFailed(true)
          return
        }
        const res = await fetch(url)
        if (!res.ok) throw new Error(`fetch failed: ${res.status}`)
        const buf = await res.arrayBuffer()
        if (cancelled) return
        if (buf.byteLength > MAX_RICH_BYTES) {
          setFailed(true)
          return
        }
        setData(buf)
      } catch {
        if (!cancelled) setFailed(true)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [source, eligible])

  const onError = useCallback(() => setFailed(true), [])

  if (!eligible || failed) return <OfficeTextFallback source={source} />
  if (!data) return <OfficeLoading />
  if (format === "docx") return <DocxView data={data} onError={onError} />
  if (format === "xlsx") return <XlsxView data={data} onError={onError} />
  return <PptxView data={data} onError={onError} />
}
