export {
  type SkillSummary,
  type SkillInstallSpec,
  type SkillStatus,
  type SkillStatusEntry,
} from "../types"

export interface SkillFileInfo {
  name: string
  size: number
  is_dir: boolean
}

export interface SkillRequires {
  bins: string[]
  any_bins?: string[]
  env: string[]
  os: string[]
  config?: string[]
  always?: boolean
  primary_env?: string
}

export interface SkillDetail {
  name: string
  description: string
  source: string
  file_path: string
  base_dir: string
  content: string
  enabled: boolean
  files: SkillFileInfo[]
  requires: SkillRequires
  skill_key?: string
  user_invocable?: boolean
  disable_model_invocation?: boolean
  command_dispatch?: string
  command_tool?: string
  install?: import("../types").SkillInstallSpec[]
  allowed_tools?: string[]
  context_mode?: string
  agent?: string
  effort?: string
  paths?: string[]
  status?: import("../types").SkillStatus
  authored_by?: string
  rationale?: string
  display?: import("../types").SkillDisplay
}

export type ToolKind = "hope" | "claude" | "codex" | "gemini" | "opencode" | "generic"
export type AppKind = Exclude<ToolKind, "generic">
export type SkillSourceType = "bundled" | "user" | "custom" | "registry"
export type SkillSourceStatus = "ready" | "empty" | "warning" | "missing" | "unavailable"
export type SkillValidationStatus = "valid" | "warning" | "invalid"
export type SkillIssueSeverity = "warning" | "error"
export type SkillPathKind = "directory" | "symlink" | "archive" | "registry"
export type InstallationState = "ready" | "attention" | "conflict" | "linked" | "external"
export type SkillPackageChannel = "local" | "zip" | "registry"

export interface SkillValidationIssue {
  code: string
  message: string
  severity: SkillIssueSeverity
}

export interface SkillSourceRecord {
  id: string
  name: string
  toolKind: ToolKind
  sourceType: SkillSourceType
  rootPath: string
  status: SkillSourceStatus
  lastIndexedAt: string | null
  issues: SkillValidationIssue[]
}

export interface SkillAppInstallState {
  app: AppKind
  installed: boolean
  state: InstallationState
  targetPath?: string
  reason?: string
}

export interface SkillUsageSnapshot {
  skillName: string
  usageCount: number
  lastUsedAt: string | null
  apps: SkillAppInstallState[]
}

export interface SkillUsageTrendPoint {
  date: string
  app: AppKind
  count: number
}

export interface SkillUsageRecentRecord {
  activatedAt: string
  app: AppKind
  skillName: string
  sessionId: string
  count: number
}

export interface SkillUsageAppBreakdown {
  app: AppKind
  count: number
}

export interface SkillPackageSummary {
  id: string
  name: string
  version?: string
  channel: SkillPackageChannel
  sourceStatus: SkillSourceStatus
  installState: InstallationState
  description?: string
  readOnly: boolean
  actions: Array<"preview" | "dryRunImport" | "install" | "update" | "export">
}

export interface SkillAppProbe {
  app: AppKind
  installed: boolean
  rootPath?: string
  status: SkillSourceStatus
}

export interface SkillDockSnapshot {
  sources: SkillSourceRecord[]
  packages: SkillPackageSummary[]
  usage: SkillUsageSnapshot[]
  usageTrend: SkillUsageTrendPoint[]
  recentUsage: SkillUsageRecentRecord[]
  usageAppBreakdown: SkillUsageAppBreakdown[]
  apps: SkillAppProbe[]
  generatedAt: string
}

export interface SkillRegistryEntry {
  id: string
  name: string
  description?: string
  sourceId: string
  sourcePath: string
  skillPath: string
  category: string
  tags: string[]
  version?: string
  updatedAt?: string
  installed: boolean
  updateAvailable: boolean
  installedState: "available" | "installed" | "updateAvailable"
  actions: Array<"preview" | "install" | "update" | "export">
}

export interface SkillRegistrySnapshot {
  entries: SkillRegistryEntry[]
  sources: SkillSourceRecord[]
  generatedAt: string
}

export interface SkillRemoteMarketEntry {
  id: string
  sourceId: string
  sourceName: string
  name: string
  source: string
  sourceType: "github"
  skillPath: string
  rawUrl: string
  description: string
  author: string
  category: string
  tags: string[]
  rating: number
  downloadCount: number
  updatedAt?: string
  featured: boolean
  compatibleApps: AppKind[]
  marketVersion?: string
  installedVersion?: string
  marketHash?: string
  installedHash?: string
  comparisonBasis: "notInstalled" | "version" | "hash" | "unavailable"
  installed: boolean
  updateAvailable: boolean
  updateReason?: string
  installedState: "available" | "installed" | "updateAvailable"
  actions: Array<"inspect" | "install" | "update">
}

export interface SkillRemoteMarketSource {
  id: string
  name: string
  url: string
  license: string
  readOnly: boolean
  sourceType: "clawhub-lock"
  status: "ready" | "error"
  error?: string
  entryCount: number
  categoryCounts: Record<string, number>
  installedCount: number
  updateCount: number
}

export interface SkillRemoteMarketSnapshot {
  sources: SkillRemoteMarketSource[]
  entries: SkillRemoteMarketEntry[]
  generatedAt: string
}

export interface SkillMarketHubConfigFile {
  defaultHubId: string
  hubs: SkillMarketHubConfig[]
  registries: SkillMarketRegistryConfig[]
}

export interface SkillMarketHubConfig {
  id: string
  name: string
  baseUrl: string
  kind: "clawhub" | "skillhub" | "generic" | string
  sourceType: "clawhub-lock" | "skillhub" | "registry" | string
  tokenRef?: string
  readOnly: boolean
  enabled: boolean
  createdAt: string
  updatedAt: string
}

export interface SkillMarketRegistryConfig {
  id: string
  hubId: string
  name: string
  registryUrl: string
  enabled: boolean
  createdAt: string
  updatedAt: string
}

export interface SkillMarketHubTokenStatus {
  hubId: string
  tokenRef?: string
  hasToken: boolean
  masked?: string
}

export interface SkillMarketHubUpsertRequest {
  id?: string
  name: string
  baseUrl: string
  kind: string
  sourceType: string
  readOnly?: boolean
  enabled?: boolean
}

export interface SkillMarketRegistryUpsertRequest {
  id?: string
  hubId: string
  name: string
  registryUrl: string
  enabled?: boolean
}

export interface SkillPublishDraftRequest {
  skillName: string
  hubId: string
}

export interface SkillPublishDraft {
  ok: boolean
  status: string
  error?: string
  skillName: string
  hubId: string
  sourceId: string
  registryUrl: string
  manifest: Record<string, unknown>
  readme: string
  hash: string
  publishable: boolean
  tokenRequired: boolean
  tokenConfigured: boolean
}

export interface SkillPublishPushRequest {
  skillName: string
  hubId: string
  confirmed: boolean
}

export interface SkillPublishPushResult {
  ok: boolean
  status: string
  error?: string
  skillName: string
  sourceId: string
  registryUrl: string
  publishedAt?: string
}

export interface SkillRegistryInstallReport {
  name: string
  sourcePath: string
  targetPath: string
  installed: boolean
  updated: boolean
  backupPath?: string
}

export interface SkillRemoteMarketInstallRequest {
  name: string
  source: string
  sourceType: "github"
  skillPath: string
  marketHash?: string
  marketVersion?: string
  sourceId?: string
  sourceName?: string
}

export interface SkillRemoteMarketInstallReport {
  name: string
  source: string
  skillPath: string
  targetPath: string
  installed: boolean
  updated: boolean
  verifiedHash?: string
  backupPath?: string
}

export interface SkillAppInstallReport {
  skillName: string
  app: AppKind
  sourcePath: string
  targetPath: string
  installed: boolean
}

export interface SkillUninstallReport {
  skillName: string
  removedPath: string
  removed: boolean
}

export interface SkillUsageScanReport {
  usage: SkillUsageSnapshot[]
  usageTrend: SkillUsageTrendPoint[]
  recentUsage: SkillUsageRecentRecord[]
  usageAppBreakdown: SkillUsageAppBreakdown[]
  scannedAt: string
  source: string
}

export interface SkillZipEntryPreview {
  name: string
  size: number
  compressedSize: number
  isDir: boolean
}

export interface SkillZipDryRunReport {
  ok: boolean
  path: string
  entryCount: number
  totalUncompressedSize: number
  skillCount: number
  skillNames: string[]
  entries: SkillZipEntryPreview[]
  issues: SkillValidationIssue[]
  dryRunOnly: boolean
}

export interface SkillZipImportReport {
  imported: string[]
  renamed: Record<string, string>
  targetDir: string
  dryRun: SkillZipDryRunReport
}

export interface SkillZipExportReport {
  skillName: string
  outputPath: string
  entryCount: number
  totalUncompressedSize: number
}
