import React, { useMemo } from "react"
import {
  FileText,
  FileArchive,
  FileType,
  FileSpreadsheet,
  FileCode,
  FileAudio,
  FileVideo,
  FileImage,
  File as FileIcon,
} from "lucide-react"
import { formatBytes } from "@/lib/format"
import type { MediaItem } from "@/types/chat"
import { FileContextMenu, FileActionsMoreButton } from "@/components/chat/files/FileActionMenu"
import { useFileActions } from "@/components/chat/files/useFileActions"
import type { PreviewTarget } from "@/components/chat/files/useFilePreview"

type IconKey =
  | "image"
  | "audio"
  | "video"
  | "pdf"
  | "archive"
  | "spreadsheet"
  | "doc"
  | "code"
  | "file"

/** Pick the icon key for a given MIME (falls back to filename extension). */
function resolveIconKey(mime: string, name: string): IconKey {
  const mimeLower = mime.toLowerCase()
  if (mimeLower.startsWith("image/")) return "image"
  if (mimeLower.startsWith("audio/")) return "audio"
  if (mimeLower.startsWith("video/")) return "video"
  if (mimeLower === "application/pdf") return "pdf"
  if (
    mimeLower === "application/zip" ||
    mimeLower === "application/gzip" ||
    mimeLower === "application/x-7z-compressed" ||
    mimeLower === "application/vnd.rar" ||
    mimeLower === "application/x-tar"
  )
    return "archive"
  if (
    mimeLower === "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" ||
    mimeLower === "application/vnd.ms-excel" ||
    mimeLower === "text/csv"
  )
    return "spreadsheet"
  if (
    mimeLower === "application/vnd.openxmlformats-officedocument.wordprocessingml.document" ||
    mimeLower === "application/msword"
  )
    return "doc"
  if (
    mimeLower.startsWith("text/") ||
    mimeLower === "application/json" ||
    mimeLower === "application/xml" ||
    mimeLower === "application/javascript"
  )
    return "code"

  const ext = name.split(".").pop()?.toLowerCase()
  switch (ext) {
    case "png":
    case "jpg":
    case "jpeg":
    case "gif":
    case "webp":
    case "svg":
    case "bmp":
    case "ico":
      return "image"
    case "mp3":
    case "wav":
    case "ogg":
    case "flac":
    case "m4a":
      return "audio"
    case "mp4":
    case "mov":
    case "webm":
    case "mkv":
    case "avi":
      return "video"
    case "pdf":
      return "pdf"
    case "zip":
    case "tar":
    case "gz":
    case "tgz":
    case "7z":
    case "rar":
      return "archive"
    case "xlsx":
    case "xls":
    case "csv":
      return "spreadsheet"
    case "doc":
    case "docx":
      return "doc"
    case "ts":
    case "tsx":
    case "js":
    case "jsx":
    case "json":
    case "rs":
    case "py":
    case "go":
    case "java":
    case "kt":
    case "swift":
    case "c":
    case "cc":
    case "cpp":
    case "h":
    case "hpp":
    case "css":
    case "scss":
    case "html":
    case "xml":
    case "md":
    case "toml":
    case "yaml":
    case "yml":
    case "sh":
      return "code"
    default:
      return "file"
  }
}

export function FileMimeIcon({
  mime,
  name,
  className,
}: {
  mime: string
  name: string
  className?: string
}) {
  const key = resolveIconKey(mime, name)
  switch (key) {
    case "image":
      return <FileImage className={className} />
    case "audio":
      return <FileAudio className={className} />
    case "video":
      return <FileVideo className={className} />
    case "pdf":
      return <FileText className={className} />
    case "archive":
      return <FileArchive className={className} />
    case "spreadsheet":
      return <FileSpreadsheet className={className} />
    case "doc":
      return <FileType className={className} />
    case "code":
      return <FileCode className={className} />
    case "file":
    default:
      return <FileIcon className={className} />
  }
}

/** Downloadable file card rendered for `send_attachment` and any other tool
 *  that emits structured media items via the `__MEDIA_ITEMS__` prefix.
 *  Primary click follows the unified policy (preview / open / download by kind ×
 *  mode); right-click and the ⋯ button expose the full action menu. */
function FileCard({ item }: { item: MediaItem }) {
  const target = useMemo<PreviewTarget>(() => ({ kind: "media", item }), [item])
  const { primary, run } = useFileActions(target)

  return (
    <FileContextMenu target={target}>
      <div className="inline-flex items-center gap-2 max-w-sm rounded-md border border-border/50 bg-secondary/30 hover:bg-secondary/50 transition-colors px-2.5 py-1.5 text-xs">
        <FileMimeIcon
          mime={item.mimeType}
          name={item.name}
          className="h-4 w-4 shrink-0 text-muted-foreground"
        />
        <button
          type="button"
          onClick={() => run(primary)}
          className="flex flex-col items-start min-w-0 flex-1 text-left hover:text-foreground transition-colors"
        >
          <span className="truncate max-w-[240px] font-medium text-foreground/90">
            {item.name}
          </span>
          <span className="text-[10px] text-muted-foreground/70 tabular-nums">
            {formatBytes(item.sizeBytes)}
          </span>
        </button>
        <FileActionsMoreButton target={target} className="shrink-0" />
      </div>
    </FileContextMenu>
  )
}

export default React.memo(FileCard)
