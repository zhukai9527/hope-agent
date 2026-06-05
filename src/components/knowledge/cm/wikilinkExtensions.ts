// CodeMirror 6 extensions for the note editor (design D13): wikilink chip
// decorations, `[[`/`#` autocomplete, and broken-link lint. The underlying
// document stays plain `.md` text — decorations only style it.

import {
  autocompletion,
  type Completion,
  type CompletionContext,
  type CompletionResult,
} from "@codemirror/autocomplete"
import { linter, type Diagnostic } from "@codemirror/lint"
import { RangeSetBuilder } from "@codemirror/state"
import {
  Decoration,
  type DecorationSet,
  EditorView,
  ViewPlugin,
  type ViewUpdate,
} from "@codemirror/view"

export interface WikilinkCompletionItem {
  /** Label inserted (basename or path). */
  label: string
  /** Secondary text (e.g. full path). */
  detail?: string
}

/** Live data the extensions read on each evaluation (kept in a ref so the
 *  extensions never need rebuilding when notes/tags change). */
export interface WikilinkData {
  notes: WikilinkCompletionItem[]
  tags: string[]
  /** Normalized resolvable targets (paths + basenames, lowercased, no `.md`). */
  knownTargets: Set<string>
}

const WIKILINK_RE = /\[\[([^\]\n]+)\]\]/g

/** Normalize a wikilink target for resolution checks (mirrors the Rust resolver
 *  key: drop anchor/alias, strip `.md`, lowercase, `\`→`/`). */
export function normalizeRef(ref: string): string {
  let s = ref.trim()
  const pipe = s.indexOf("|")
  if (pipe >= 0) s = s.slice(0, pipe)
  const hash = s.indexOf("#")
  if (hash >= 0) s = s.slice(0, hash)
  s = s.trim().replace(/\\/g, "/")
  s = s.replace(/\.(md|markdown)$/i, "")
  return s.toLowerCase()
}

function isResolvable(ref: string, known: Set<string>): boolean {
  const norm = normalizeRef(ref)
  if (!norm) return false
  if (known.has(norm)) return true
  // Basename fallback for path-form refs.
  const base = norm.split("/").pop() ?? norm
  return known.has(base)
}

/** Chip-style decorations over `[[...]]`, marking broken links distinctly. */
export function wikilinkDecorations(getData: () => WikilinkData) {
  const build = (view: EditorView): DecorationSet => {
    const builder = new RangeSetBuilder<Decoration>()
    const data = getData()
    const text = view.state.doc.toString()
    let m: RegExpExecArray | null
    WIKILINK_RE.lastIndex = 0
    while ((m = WIKILINK_RE.exec(text)) !== null) {
      const start = m.index
      const end = m.index + m[0].length
      const broken = !isResolvable(m[1], data.knownTargets)
      builder.add(
        start,
        end,
        Decoration.mark({
          class: broken ? "cm-wikilink cm-wikilink-broken" : "cm-wikilink",
        }),
      )
    }
    return builder.finish()
  }

  return ViewPlugin.fromClass(
    class {
      decorations: DecorationSet
      constructor(view: EditorView) {
        this.decorations = build(view)
      }
      update(u: ViewUpdate) {
        if (u.docChanged || u.viewportChanged) {
          this.decorations = build(u.view)
        }
      }
    },
    { decorations: (v) => v.decorations },
  )
}

/** `[[` note autocomplete + `#` tag autocomplete. */
export function wikilinkCompletion(getData: () => WikilinkData) {
  return autocompletion({
    override: [
      (ctx: CompletionContext): CompletionResult | null => {
        const data = getData()
        // `[[note` — match the text after the last unclosed `[[`.
        const wiki = ctx.matchBefore(/\[\[([^\]\n]*)$/)
        if (wiki) {
          const from = wiki.from + 2 // after `[[`
          return {
            from,
            options: data.notes.map((n) => ({
              label: n.label,
              detail: n.detail,
              type: "class",
              apply: (view: EditorView, _c: Completion, afrom: number, ato: number) => {
                // The editor auto-closes `[[` into `[[]]`, so a `]]` may already
                // sit after the caret — don't insert a second pair.
                const hasClose = view.state.sliceDoc(ato, ato + 2) === "]]"
                const insert = hasClose ? n.label : `${n.label}]]`
                view.dispatch({
                  changes: { from: afrom, to: ato, insert },
                  selection: { anchor: afrom + n.label.length + 2 },
                })
              },
            })),
            validFor: /^[^\]\n]*$/,
          }
        }
        // `#tag`
        const tag = ctx.matchBefore(/(^|\s)#([\p{L}\p{N}_/-]*)$/u)
        if (tag) {
          const hashIdx = tag.text.lastIndexOf("#")
          const from = tag.from + hashIdx + 1
          return {
            from,
            options: data.tags.map((t) => ({ label: t, type: "keyword" })),
            validFor: /^[\p{L}\p{N}_/-]*$/u,
          }
        }
        return null
      },
    ],
  })
}

/** Lint that flags `[[ref]]` not resolving to a known note. */
export function brokenLinkLinter(getData: () => WikilinkData) {
  return linter((view): Diagnostic[] => {
    const data = getData()
    const diags: Diagnostic[] = []
    const text = view.state.doc.toString()
    let m: RegExpExecArray | null
    WIKILINK_RE.lastIndex = 0
    while ((m = WIKILINK_RE.exec(text)) !== null) {
      if (!isResolvable(m[1], data.knownTargets)) {
        diags.push({
          from: m.index,
          to: m.index + m[0].length,
          severity: "warning",
          message: `Broken link: no note matches "${m[1].trim()}"`,
        })
      }
    }
    return diags
  })
}

/** Theme for wikilink chips. */
export const wikilinkTheme = EditorView.baseTheme({
  ".cm-wikilink": {
    color: "var(--primary, #6366f1)",
    backgroundColor: "color-mix(in srgb, var(--primary, #6366f1) 12%, transparent)",
    borderRadius: "3px",
    padding: "0 2px",
  },
  ".cm-wikilink-broken": {
    color: "var(--destructive, #ef4444)",
    backgroundColor: "color-mix(in srgb, var(--destructive, #ef4444) 12%, transparent)",
    textDecoration: "underline wavy",
  },
})

/** Build the normalized known-target set from a note list. */
export function buildKnownTargets(notes: { relPath: string; title: string }[]): Set<string> {
  const set = new Set<string>()
  for (const n of notes) {
    const rel = n.relPath.replace(/\\/g, "/").replace(/\.(md|markdown)$/i, "").toLowerCase()
    set.add(rel)
    const base = rel.split("/").pop()
    if (base) set.add(base)
    if (n.title) set.add(n.title.trim().toLowerCase())
  }
  return set
}
