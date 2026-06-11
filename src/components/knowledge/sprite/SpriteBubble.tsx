import { useTranslation } from "react-i18next"
import { CornerDownLeft, X } from "lucide-react"

import { AgentAvatarBadge, type AgentSelectAgent } from "@/components/common/AgentSelectDisplay"
import { IconTip } from "@/components/ui/tooltip"
import { cn } from "@/lib/utils"
import type { SpriteCategory, SpriteSuggestion } from "@/types/knowledge"

const CATEGORY_STYLE: Record<SpriteCategory, string> = {
  writing: "bg-blue-500/15 text-blue-600 dark:text-blue-300",
  review: "bg-purple-500/15 text-purple-600 dark:text-purple-300",
  encourage: "bg-rose-500/15 text-rose-600 dark:text-rose-300",
  remind: "bg-amber-500/15 text-amber-600 dark:text-amber-300",
  connect: "bg-emerald-500/15 text-emerald-600 dark:text-emerald-300",
}

/**
 * Transient "sprite" bubble shown above the composer (Phase 2). A soft-glow card
 * with the agent avatar, a category badge, the suggestion, and dismiss / respond
 * actions. Never persisted into the message stream.
 */
export default function SpriteBubble({
  suggestion,
  agent,
  onDismiss,
  onRespond,
}: {
  suggestion: SpriteSuggestion
  agent?: AgentSelectAgent | null
  onDismiss: () => void
  onRespond: (text: string) => void
}) {
  const { t } = useTranslation()
  const cat = suggestion.category

  return (
    <div className="relative mx-2 mb-2 animate-in fade-in-0 slide-in-from-bottom-1 duration-200">
      <div
        className={cn(
          "rounded-2xl border border-primary/20 bg-primary/[0.06] p-2.5 pr-8",
          "shadow-[0_0_20px_-6px] shadow-primary/30",
        )}
      >
        <div className="flex items-start gap-2">
          <AgentAvatarBadge agent={agent ?? null} size="sm" className="mt-0.5 ring-2 ring-primary/20" />
          <div className="min-w-0 flex-1">
            <span
              className={cn(
                "inline-block rounded-full px-1.5 py-0.5 text-[10px] font-medium leading-none",
                CATEGORY_STYLE[cat],
              )}
            >
              {t(`knowledge.sprite.category.${cat}`, cat)}
            </span>
            <p className="mt-1 max-h-48 overflow-y-auto whitespace-pre-wrap break-words text-[13px] leading-relaxed text-foreground/90">
              {suggestion.text}
            </p>
            <button
              type="button"
              onClick={() => onRespond(suggestion.text)}
              className="mt-1.5 inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[11px] font-medium text-primary transition-colors hover:bg-primary/10"
            >
              <CornerDownLeft className="h-3 w-3" />
              {t("knowledge.sprite.respond", "Respond")}
            </button>
          </div>
        </div>
      </div>
      <IconTip label={t("knowledge.sprite.dismiss", "Dismiss")}>
        <button
          type="button"
          onClick={onDismiss}
          aria-label={t("knowledge.sprite.dismiss", "Dismiss")}
          className="absolute right-1.5 top-1.5 flex h-5 w-5 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-secondary hover:text-foreground"
        >
          <X className="h-3 w-3" />
        </button>
      </IconTip>
    </div>
  )
}
