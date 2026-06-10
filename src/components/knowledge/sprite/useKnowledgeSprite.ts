import { useCallback, useEffect, useRef, useState } from "react"

import { getTransport } from "@/lib/transport-provider"
import { logger } from "@/lib/logger"
import type { SpriteConfig, SpriteSuggestion } from "@/types/knowledge"

const DOC_SEND_CAP = 8000
const EDIT_SEND_CAP = 1200

/** Cheap "what changed" diff: strip the common prefix/suffix, return the middle
 *  of the new text (net-added region) + how many chars changed. */
function diffMiddle(prev: string, next: string): { changed: number; edit: string } {
  if (prev === next) return { changed: 0, edit: "" }
  let p = 0
  const max = Math.min(prev.length, next.length)
  while (p < max && prev[p] === next[p]) p++
  let s = 0
  while (s < max - p && prev[prev.length - 1 - s] === next[next.length - 1 - s]) s++
  const edit = next.slice(p, next.length - s)
  // Effective change magnitude. Use max (not sum) of the net length delta and
  // the edited middle: for a pure insertion both are equal, so summing would
  // double-count and trip the threshold at half the configured chars.
  const changed = Math.max(Math.abs(next.length - prev.length), edit.length)
  return { changed, edit }
}

interface Opts {
  kbId: string | null
  notePath: string | null
  sessionId: string | null
  agentId: string
  /** Increments on every editor change (push signal to debounce on). */
  editorRevision: number
  /** Pulls the editor's current text (lazy — only read on fire). */
  getEditorValue: () => string
  /** Recent conversation turns (newest last) for the conversation sense. */
  getRecentMessages: () => Array<{ role: string; text: string }>
  /** Panel actually visible. */
  active: boolean
}

/**
 * Knowledge-space sprite / inspiration mode (Phase 2). Owns:
 * - the enable flag (`SpriteConfig.enabled`), toggled directly from the chat-bar
 *   button and persisted via `sprite_config_set_cmd` (no separate settings switch),
 * - the edit-idle trigger that posts `kb_sprite_observe_cmd`,
 * - the `sprite:suggestion` listener that surfaces a transient bubble.
 * The backend does all throttling + the LLM call; this only fires observations
 * and renders what comes back. Other tuning (idle / cooldown / senses) lives in
 * Settings → Knowledge Space.
 */
export function useKnowledgeSprite(opts: Opts) {
  const { kbId, notePath, sessionId, agentId, editorRevision, active } = opts

  const [config, setConfig] = useState<SpriteConfig | null>(null)
  const [suggestion, setSuggestion] = useState<SpriteSuggestion | null>(null)

  // Stable readers so the debounce effect doesn't re-arm on every render.
  // Synced in an effect (not during render) so the debounce timer always pulls
  // the latest closures without re-arming.
  const getEditorValueRef = useRef(opts.getEditorValue)
  const getRecentMessagesRef = useRef(opts.getRecentMessages)
  // `null` = baseline not yet established for the current note. Re-established
  // lazily on the next idle tick from the actually-loaded content.
  const lastObservedRef = useRef<string | null>(null)
  useEffect(() => {
    getEditorValueRef.current = opts.getEditorValue
    getRecentMessagesRef.current = opts.getRecentMessages
  })

  const enabled = config?.enabled ?? false
  const idleSecs = config?.idleEditSecs ?? 8
  const minChange = config?.minChangeChars ?? 80
  const armed = enabled && active && !!kbId && !!notePath

  // Load the sprite config when shown + whenever it changes elsewhere (settings).
  useEffect(() => {
    if (!active) return
    const load = () =>
      getTransport()
        .call<SpriteConfig>("sprite_config_get_cmd")
        .then(setConfig)
        .catch((e) => logger.warn("knowledge", "useKnowledgeSprite::config", "load failed", e))
    void load()
    return getTransport().listen("config:changed", (raw) => {
      const p = raw as { category?: string } | null
      if (p?.category && p.category !== "sprite") return
      void load()
    })
  }, [active])

  // Chat-bar toggle: flip + persist `enabled` (optimistic local update). Needs
  // the config loaded first (the button is hidden/disabled until then).
  const setEnabled = useCallback(
    (on: boolean) => {
      if (!config) return
      const next = { ...config, enabled: on }
      setConfig(next)
      getTransport()
        .call<SpriteConfig>("sprite_config_set_cmd", { config: next })
        .then(setConfig)
        .catch((e) => logger.warn("knowledge", "useKnowledgeSprite::toggle", "save failed", e))
    },
    [config],
  )

  const dismiss = useCallback(() => setSuggestion(null), [])

  // Switching notes invalidates the diff baseline. We intentionally do NOT read
  // the editor here: this child effect runs before the parent's editor-value
  // mirror updates (and the new note's content loads async), so reading now
  // would capture the *previous* note's text and the next idle tick would diff
  // the whole new doc against it — a false observation with zero edits. Reset to
  // `null` and let the trigger establish the baseline from the loaded content.
  // A stale suggestion from the previous note is hidden by the notePath gate at
  // return time rather than cleared here.
  useEffect(() => {
    lastObservedRef.current = null
  }, [notePath])

  // Listen for backend suggestions, filtered to the current note (+ session).
  useEffect(() => {
    if (!armed) return
    const unlisten = getTransport().listen("sprite:suggestion", (raw) => {
      const s = raw as SpriteSuggestion | null
      if (!s || s.notePath !== notePath) return
      if (s.sessionId && sessionId && s.sessionId !== sessionId) return
      setSuggestion(s)
    })
    return unlisten
  }, [armed, notePath, sessionId])

  // Edit-idle trigger: debounce on editorRevision, fire when changed enough.
  useEffect(() => {
    if (!armed) return
    const handle = window.setTimeout(() => {
      const doc = getEditorValueRef.current() ?? ""
      // First tick after opening/switching a note just establishes the baseline
      // from the loaded content — never fire on the diff between two notes.
      if (lastObservedRef.current === null) {
        lastObservedRef.current = doc
        return
      }
      const { changed, edit } = diffMiddle(lastObservedRef.current, doc)
      if (changed < minChange) return
      lastObservedRef.current = doc
      getTransport()
        .call("kb_sprite_observe_cmd", {
          params: {
            sessionId: sessionId ?? undefined,
            kbId,
            notePath,
            agentId,
            docContent: doc.slice(0, DOC_SEND_CAP),
            recentEdit: edit ? edit.slice(0, EDIT_SEND_CAP) : undefined,
            recentMessages: getRecentMessagesRef.current().slice(-6),
          },
        })
        .catch((e) => logger.warn("knowledge", "useKnowledgeSprite::observe", "post failed", e))
    }, idleSecs * 1000)
    return () => window.clearTimeout(handle)
  }, [armed, editorRevision, idleSecs, minChange, sessionId, kbId, notePath, agentId])

  // Only surface a suggestion that belongs to the currently-open note.
  const visibleSuggestion = suggestion && suggestion.notePath === notePath ? suggestion : null

  return {
    /** Whether sprite mode is on (toggled from the chat bar). `null` config = not loaded yet. */
    enabled,
    ready: config != null,
    setEnabled,
    suggestion: visibleSuggestion,
    dismiss,
  }
}
