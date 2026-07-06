export const CHAT_INPUT_OVERFLOW_ACTION_IDS = [
  "working-dir",
  "attach-files",
  "slash-command",
] as const

export type ChatInputOverflowActionId = (typeof CHAT_INPUT_OVERFLOW_ACTION_IDS)[number]

export function getChatInputOverflowActionIds(): ChatInputOverflowActionId[] {
  return [...CHAT_INPUT_OVERFLOW_ACTION_IDS]
}

export const CHAT_INPUT_INLINE_ADD_ACTIONS_CLASS = "contents"
export const CHAT_INPUT_OVERFLOW_MENU_CLASS = "hidden"
// Measured against the chat input container, not the viewport. Right-side
// panels can squeeze the chat column while the app window remains wide.
//
// Progressive collapse into the "+" menu as the toolbar narrows. Each tier is a
// strict subset of the wider one (1040 ⊃ 970 ⊃ 660 ⊃ 510), so a narrower width
// always implies every wider collapse. The floor — "+" · model picker ·
// awareness icon (when enabled) · send/stop — is never collapsed, and send/stop
// never wraps onto its own row.
//
// The breakpoints are the container (input-dock) width at which the control that
// collapses at that tier would otherwise wrap. They are derived from a per-
// control width budget rather than picked by feel — the ModelPicker truncates
// (`min-w-0 max-w-[220px] truncate`) so it never forces a wrap; the wrap risk is
// the `whitespace-nowrap` label controls. Widths in px, worst-case English
// active labels, with the flex `gap-1` (4px) between items folded in:
//
//   tail (voice/send col ~104 + grid gap 8 + px-2 16 + border/rounding ~6) ≈ 134
//   "+" trigger 32 · ModelPicker ~160 · Awareness icon ~32
//   Permission ~131 · Sandbox ~146
//   Knowledge ~36 · Goal ~54 · Workflow menu ~142 · Plan ~66
//   Add-actions row 108 (extra +76 over "+")
//
// Cumulative container width needed to keep each control inline (→ breakpoint):
//   floor  ("+", model, awareness)                         ≈ 366
//   + permission                                            ≈ 501 → 510
//   + sandbox                                               ≈ 651 → 660
//   + knowledge + goal + workflow + plan                    ≈ 949 → 970
//   + add-actions expanded ("+" → 3 inline buttons)         ≈ 1025 → 1040
//
// The larger tight tier is deliberate: Goal + Workflow are semantic mode
// controls with labels, not icon-only tools, so they move into "+" before the
// toolbar gets close to wrapping.
export const CHAT_INPUT_OVERFLOW_BREAKPOINT_PX = 1040
export const CHAT_INPUT_TIGHT_TOOLBAR_BREAKPOINT_PX = 970
export const CHAT_INPUT_SANDBOX_COLLAPSE_BREAKPOINT_PX = 660
export const CHAT_INPUT_PERMISSION_COLLAPSE_BREAKPOINT_PX = 510
