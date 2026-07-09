export const LOOP_SLASH_CONTROL_WORDS = new Set([
  "status",
  "list",
  "show",
  "help",
  "pause",
  "resume",
  "stop",
  "cancel",
])

function readLoopSlashArgs(commandText: string | undefined): string | null {
  const trimmed = commandText?.trim() ?? ""
  const match = /^\/loop(?=\s|$)/i.exec(trimmed)
  if (!match) return null
  return trimmed.slice(match[0].length).trim()
}

export function parseLoopCreateSlashCommand(commandText: string | undefined): string | null {
  const args = readLoopSlashArgs(commandText)
  if (!args) return null
  const first = args.split(/\s+/)[0]?.toLowerCase() ?? ""
  if (LOOP_SLASH_CONTROL_WORDS.has(first)) return null
  return args
}

export function isLoopCreateSlashCommand(commandText: string | undefined): boolean {
  return parseLoopCreateSlashCommand(commandText) != null
}

export function loopSlashCommandDisplay(commandText: string): {
  content: string
  mode?: "loop"
} {
  const args = readLoopSlashArgs(commandText)
  if (args == null) return { content: commandText }

  const first = args.split(/\s+/)[0]?.toLowerCase() ?? ""
  const content =
    args.length === 0
      ? "Start self-paced loop"
      : first === "status" || first === "list" || first === "show"
        ? "Show loops"
        : first === "pause"
          ? "Pause loop"
          : first === "resume"
            ? "Resume loop"
            : first === "stop" || first === "cancel"
              ? "Stop loop"
              : first === "help"
                ? "Loop help"
                : args
  return { content: content || "Loop", mode: "loop" }
}
