/**
 * Type definitions for the Project feature.
 *
 * Mirrors `crates/ha-core/src/project/types.rs` with serde camelCase naming.
 */

export interface Project {
  id: string
  name: string
  description?: string | null
  /** Optional logo stored as a `data:image/...;base64,...` URL. */
  logo?: string | null
  /** Tailwind-ish color name (e.g. "amber", "violet"). */
  color?: string | null
  defaultAgentId?: string | null
  defaultModelId?: string | null
  /**
   * Default working directory for sessions in this project. Used as a
   * fallback when the session itself has no `workingDir` set.
   */
  workingDir?: string | null
  /** Unix milliseconds. */
  createdAt: number
  updatedAt: number
  /** Sidebar sort key. Lower values render earlier. */
  sortOrder: number
  archived: boolean
}

/** Project enriched with counts that drive the sidebar badges. */
export interface ProjectMeta extends Project {
  sessionCount: number
  unreadCount: number
}

export interface ProjectInstructionsStats {
  path: string
  lineCount: number
  sizeBytes: number
  empty: boolean
}

export interface ProjectOverviewSummary {
  sessionCount: number
  recentSessions: import("./chat").SessionMeta[]
  /** null when the Core Memory repository could not be read. */
  autoMemoryTopicCount?: number | null
  /** Effective-active structured memories in this project scope; null when unavailable. */
  activeClaimCount?: number | null
  /** null when AGENTS.md could not be inspected or re-created. */
  instructions?: ProjectInstructionsStats | null
}

/** Snapshot returned by the root AGENTS.md owner endpoints. */
export interface ProjectInstructionsFile {
  path: string
  content: string
  contentHash: string
  created: boolean
}

/** Concurrency-checked AGENTS.md draft submitted with project metadata. */
export interface ProjectInstructionsDraft {
  content: string
  expectedFileHash: string
}

export interface ProjectWorkflowDiscovery {
  projectId: string
  workingDir?: string | null
  workflowsDir: string
  exists: boolean
  missingFiles: string[]
  templates: ProjectWorkflowTemplateSummary[]
  modes: ProjectWorkflowModeSummary[]
  fixedArtifacts: ProjectWorkflowFixedArtifact[]
  verificationCommands: ProjectWorkflowVerificationCommand[]
}

export interface ProjectWorkflowTemplateSummary {
  id: string
  name: string
  taskTypes: string[]
  phaseCount: number
  modes: string[]
  fixedArtifactsCount: number
  sourceFiles: string[]
}

export interface ProjectWorkflowModeSummary {
  id: string
  name: string
  sourceFiles: string[]
}

export interface ProjectWorkflowFixedArtifact {
  id: string
  name: string
  path?: string | null
  sourceFiles: string[]
}

export interface ProjectWorkflowVerificationCommand {
  id: string
  command: string
  sourceFiles: string[]
}

export interface ProjectWorkflowPreviewInput {
  projectId: string
  templateId: string
  mode?: string | null
  taskType?: string | null
}

export interface ProjectWorkflowPreview {
  projectId: string
  templateId: string
  mode?: string | null
  taskType?: string | null
  phases: ProjectWorkflowPhasePreview[]
  fixedArtifacts: ProjectWorkflowFixedArtifact[]
  requiredInteractions: ProjectWorkflowRequiredInteraction[]
  verificationCommands: ProjectWorkflowVerificationCommand[]
  sourceFiles: string[]
}

export interface ProjectWorkflowPhasePreview {
  id: string
  name: string
  requiredInteractions: ProjectWorkflowRequiredInteraction[]
  fixedArtifacts: ProjectWorkflowFixedArtifact[]
  verificationCommands: ProjectWorkflowVerificationCommand[]
}

export interface ProjectWorkflowRequiredInteraction {
  kind: string
  prompt: string
}

export interface CreateProjectInput {
  name: string
  description?: string | null
  /** Data URL (e.g. `data:image/webp;base64,...`). */
  logo?: string | null
  color?: string | null
  defaultAgentId?: string | null
  defaultModelId?: string | null
  /** Optional default working directory for sessions in this project. */
  workingDir?: string | null
}

/**
 * Patch DTO. `undefined` means "don't touch this field"; empty string is
 * treated by the backend as "clear this field".
 */
export interface UpdateProjectInput {
  name?: string
  description?: string
  /** Data URL, or empty string to clear the existing logo. */
  logo?: string
  color?: string
  defaultAgentId?: string
  defaultModelId?: string
  /** Empty string clears the project default working directory. */
  workingDir?: string
  archived?: boolean
}

export type ProjectMemoryType = "feedback" | "project" | "reference" | "user"

/** One topic from the machine-local project auto-memory directory. */
export interface ProjectMemoryEntry {
  fileName: string
  name: string
  description: string
  memoryType: ProjectMemoryType
  sizeBytes: number
}

export interface ProjectMemoryFile extends ProjectMemoryEntry {
  /** Markdown body only; frontmatter is represented by the fields above. */
  content: string
  /** BLAKE3 of the raw on-disk Markdown file, used for stale-write protection. */
  fileHash: string
}

export interface ProjectMemoryWriteInput {
  /** Present when updating an existing topic; omitted when creating one. */
  fileName?: string
  /** Required with fileName when overwriting an existing topic. */
  expectedFileHash?: string
  name: string
  description: string
  memoryType: ProjectMemoryType
  content: string
}
