/**
 * Render a plain string with `[@label](#skill:name)` tokens replaced by inline
 * skill chips, for compact single-line surfaces (the timeline sticky-anchor
 * pill) that aren't full markdown but should still show the same chip as the
 * message bubble. Non-markdown: only skill tokens become chips; everything else
 * stays literal text. Unknown / non-allowlisted ids degrade to `@label`.
 */

import { Fragment, type ReactNode } from "react"

import { AgentMentionChip } from "../agent-mention/AgentMentionChip"
import { parseAgentMentions } from "../agent-mention/agentTokens"
import { SkillMentionChip } from "./SkillMentionChip"
import { isSkillMentionName, parseSkillMentions } from "./skillTokens"

export function SkillMentionText({ text }: { text: string }) {
  const spans = [
    ...parseSkillMentions(text).map((span) => ({ ...span, kind: "skill" as const })),
    ...parseAgentMentions(text).map((span) => ({ ...span, kind: "agent" as const })),
  ].sort((a, b) => a.start - b.start)
  if (spans.length === 0) return <>{text}</>

  const out: ReactNode[] = []
  let cursor = 0
  spans.forEach((span, i) => {
    if (span.start < cursor) return
    if (span.start > cursor) {
      out.push(<Fragment key={`t-${i}`}>{text.slice(cursor, span.start)}</Fragment>)
    }
    if (span.kind === "skill" && isSkillMentionName(span.name)) {
      out.push(<SkillMentionChip key={`s-${i}`} name={span.name} />)
    } else if (span.kind === "agent") {
      out.push(<AgentMentionChip key={`a-${i}`} agentId={span.agentId} fallbackName={span.label} />)
    } else {
      out.push(<Fragment key={`f-${i}`}>{`@${span.label}`}</Fragment>)
    }
    cursor = span.end
  })
  if (cursor < text.length) out.push(<Fragment key="tail">{text.slice(cursor)}</Fragment>)
  return <>{out}</>
}
