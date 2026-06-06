// CodeMirror 6 "live preview" decorations for the note editor (Phase 3, D13).
//
// This is the deliverable for the "visual editor" evaluation: rather than bolting
// a ProseMirror WYSIWYG engine (Milkdown/Tiptap) on top — which would round-trip
// Markdown⇄JSON and quietly rewrite whitespace / frontmatter / wikilinks, breaking
// the `.md`-is-truth contract, D14 offsets, `note_patch` old/new matching and the
// stale-write BLAKE3 guard — we hide the *syntax markers* in place, exactly like
// Obsidian's own (CodeMirror-based) live preview. The document stays plain `.md`;
// only the rendering changes. Touching a line reveals its raw Markdown for editing.
//
// Images + math are handled by `notePreviewWidgets` (previewExtensions.ts); this
// field skips their spans (and fenced/inline code) so the two never produce
// overlapping replace decorations.

import { syntaxTree } from "@codemirror/language"
import { type EditorState, type Range, StateField } from "@codemirror/state"
import { Decoration, type DecorationSet, EditorView, WidgetType } from "@codemirror/view"

import { previewMatchRanges } from "./previewExtensions"

// Same snappiness cap as the inline-preview field: the decoration set is rebuilt
// on every edit / cursor move and scans the whole doc, so skip very large notes.
const MAX_LIVE_DOC = 100_000

/** A `•` standing in for a `-`/`*`/`+` bullet marker. */
class BulletWidget extends WidgetType {
  eq() {
    return true
  }
  toDOM() {
    const span = document.createElement("span")
    span.className = "cm-live-bullet"
    span.textContent = "•"
    return span
  }
  ignoreEvent() {
    return false
  }
}

const HEADER_LEVEL_RE = /^ATXHeading([1-6])$/
const BULLET_RE = /^[-*+]$/

const hidden = Decoration.replace({})
const bullet = Decoration.replace({ widget: new BulletWidget() })

function buildLive(state: EditorState): DecorationSet {
  if (state.doc.length > MAX_LIVE_DOC) return Decoration.none

  // Lines touched by any selection range show their raw Markdown (Obsidian-style
  // reveal). Editing a heading/bold/etc. then always sees the real characters.
  const activeLines = new Set<number>()
  for (const r of state.selection.ranges) {
    const a = state.doc.lineAt(r.from).number
    const b = state.doc.lineAt(r.to).number
    for (let ln = a; ln <= b; ln++) activeLines.add(ln)
  }

  // Spans owned by the image/math inline-preview field — never decorate inside
  // them (would overlap a replace and throw).
  const protectedRanges = previewMatchRanges(state.doc.toString())
  const isProtected = (from: number, to: number) =>
    protectedRanges.some(([pf, pt]) => from < pt && pf < to)

  const decos: Range<Decoration>[] = []
  const onActiveLine = (pos: number) => activeLines.has(state.doc.lineAt(pos).number)

  syntaxTree(state).iterate({
    enter: (node) => {
      const { name, from, to } = node
      // Don't descend into code / images: their content is literal, and images
      // are rendered by the widget field. Returning false skips the subtree.
      if (name === "FencedCode" || name === "CodeBlock" || name === "Image") {
        return false
      }
      if (isProtected(from, to) || onActiveLine(from)) return

      switch (name) {
        case "HeaderMark": {
          // Only ATX heading markers (skip the Setext underline form, whose
          // HeaderMark sits on its own line). Hide `#`s + the one trailing space,
          // and size the heading text via a mark on the rest of the line.
          const parent = node.node.parent
          const lvl = parent ? HEADER_LEVEL_RE.exec(parent.name)?.[1] : undefined
          if (!lvl || !parent) return
          const line = state.doc.lineAt(from)
          if (from !== line.from + leadingSpaces(line.text)) return // closing `#`s
          const afterMark =
            to < state.doc.length && state.doc.sliceString(to, to + 1) === " " ? to + 1 : to
          decos.push(hidden.range(from, afterMark))
          if (afterMark < line.to) {
            decos.push(
              Decoration.mark({ class: `cm-live-h${lvl}` }).range(afterMark, line.to),
            )
          }
          break
        }
        case "EmphasisMark":
          decos.push(hidden.range(from, to))
          break
        case "StrikethroughMark":
          decos.push(hidden.range(from, to))
          break
        case "CodeMark":
          decos.push(hidden.range(from, to))
          break
        case "Emphasis":
          decos.push(Decoration.mark({ class: "cm-live-em" }).range(from, to))
          break
        case "StrongEmphasis":
          decos.push(Decoration.mark({ class: "cm-live-strong" }).range(from, to))
          break
        case "Strikethrough":
          decos.push(Decoration.mark({ class: "cm-live-strike" }).range(from, to))
          break
        case "InlineCode":
          decos.push(Decoration.mark({ class: "cm-live-code" }).range(from, to))
          break
        case "ListMark": {
          // Render `-`/`*`/`+` as a real bullet; leave ordered-list numbers alone.
          if (BULLET_RE.test(state.doc.sliceString(from, to))) {
            decos.push(bullet.range(from, to))
          }
          break
        }
        case "QuoteMark": {
          // Hide `>` + trailing space; tint the quoted remainder of the line.
          const line = state.doc.lineAt(from)
          const afterMark =
            to < state.doc.length && state.doc.sliceString(to, to + 1) === " " ? to + 1 : to
          decos.push(hidden.range(from, afterMark))
          if (afterMark < line.to) {
            decos.push(Decoration.mark({ class: "cm-live-quote" }).range(afterMark, line.to))
          }
          break
        }
      }
    },
  })

  // RangeSet requires ascending `from`, and for equal `from` the marks must be
  // ordered by startSide; `Decoration.set(arr, true)` sorts for us.
  return Decoration.set(decos, true)
}

function leadingSpaces(s: string): number {
  let n = 0
  while (n < s.length && s[n] === " ") n++
  return n
}

/** Live-preview decorations (syntax-marker hiding). StateField so replacements may
 *  cover line-spanning constructs without the plugin-decoration restriction. */
export function noteLiveDecorations() {
  return StateField.define<DecorationSet>({
    create: (state) => buildLive(state),
    update: (deco, tr) =>
      tr.docChanged || tr.selection ? buildLive(tr.state) : deco,
    provide: (f) => EditorView.decorations.from(f),
  })
}

/** Visual styling for the hidden-marker spans. */
export const noteLiveTheme = EditorView.baseTheme({
  ".cm-live-strong": { fontWeight: "700" },
  ".cm-live-em": { fontStyle: "italic" },
  ".cm-live-strike": { textDecoration: "line-through", opacity: "0.7" },
  ".cm-live-code": {
    fontFamily:
      "ui-monospace, SFMono-Regular, 'SF Mono', Menlo, Consolas, 'Liberation Mono', monospace",
    backgroundColor: "rgba(127,127,127,0.16)",
    borderRadius: "3px",
    padding: "0.05em 0.3em",
  },
  ".cm-live-quote": { fontStyle: "italic", opacity: "0.8" },
  ".cm-live-bullet": { opacity: "0.6" },
  ".cm-live-h1": { fontSize: "1.6em", fontWeight: "700", lineHeight: "1.3" },
  ".cm-live-h2": { fontSize: "1.4em", fontWeight: "700", lineHeight: "1.3" },
  ".cm-live-h3": { fontSize: "1.2em", fontWeight: "700", lineHeight: "1.3" },
  ".cm-live-h4": { fontSize: "1.1em", fontWeight: "700" },
  ".cm-live-h5": { fontSize: "1.05em", fontWeight: "700" },
  ".cm-live-h6": { fontSize: "1em", fontWeight: "700", opacity: "0.85" },
})
