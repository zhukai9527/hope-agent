import { getTransport } from "@/lib/transport-provider"
import type { SkillStatusEntry, SkillSummary } from "../types"
import type {
  SkillDetail,
  SkillDockSnapshot,
  SkillAppInstallReport,
  SkillMarketHubConfigFile,
  SkillMarketHubTokenStatus,
  SkillMarketHubUpsertRequest,
  SkillMarketRegistryUpsertRequest,
  SkillPublishDraft,
  SkillPublishDraftRequest,
  SkillPublishPushRequest,
  SkillPublishPushResult,
  SkillRegistryInstallReport,
  SkillRegistrySnapshot,
  SkillRemoteMarketEntry,
  SkillRemoteMarketInstallReport,
  SkillRemoteMarketInstallRequest,
  SkillRemoteMarketSnapshot,
  SkillUninstallReport,
  SkillUsageScanReport,
  SkillZipDryRunReport,
  SkillZipExportReport,
  SkillZipImportReport,
} from "./types"

export interface SkillsManagerSnapshot {
  skills: SkillSummary[]
  extraDirs: string[]
  skillEnvCheck: boolean
  envStatus: Record<string, Record<string, boolean>>
  skillStatuses: SkillStatusEntry[]
}

export async function loadSkillsManagerSnapshot(): Promise<SkillsManagerSnapshot> {
  const [skills, extraDirs, skillEnvCheck, envStatus, skillStatuses] = await Promise.all([
    getTransport().call<SkillSummary[]>("get_skills"),
    getTransport().call<string[]>("get_extra_skills_dirs"),
    getTransport().call<boolean>("get_skill_env_check"),
    getTransport().call<Record<string, Record<string, boolean>>>("get_skills_env_status"),
    getTransport().call<SkillStatusEntry[]>("get_skills_status"),
  ])
  return { skills, extraDirs, skillEnvCheck, envStatus, skillStatuses }
}

export async function reloadSkillsManagerSnapshot(): Promise<SkillsManagerSnapshot> {
  const [skills, extraDirs, skillEnvCheck, envStatus, skillStatuses] = await Promise.all([
    getTransport().call<SkillSummary[]>("reload_skills"),
    getTransport().call<string[]>("get_extra_skills_dirs"),
    getTransport().call<boolean>("get_skill_env_check"),
    getTransport().call<Record<string, Record<string, boolean>>>("get_skills_env_status"),
    getTransport().call<SkillStatusEntry[]>("get_skills_status"),
  ])
  return { skills, extraDirs, skillEnvCheck, envStatus, skillStatuses }
}

export async function loadSkillDetail(name: string): Promise<{
  detail: SkillDetail
  maskedEnv: Record<string, string>
}> {
  const [detail, maskedEnv] = await Promise.all([
    getTransport().call<SkillDetail>("get_skill_detail", { name }),
    getTransport().call<Record<string, string>>("get_skill_env", { name }),
  ])
  return { detail, maskedEnv }
}

export function setSkillEnabled(name: string, enabled: boolean): Promise<void> {
  return getTransport().call("toggle_skill", { name, enabled })
}

export function addSkillsDirectory(dir: string): Promise<void> {
  return getTransport().call("add_extra_skills_dir", { dir })
}

export function removeSkillsDirectory(dir: string): Promise<void> {
  return getTransport().call("remove_extra_skills_dir", { dir })
}

export function setSkillEnvCheckEnabled(enabled: boolean): Promise<void> {
  return getTransport().call("set_skill_env_check", { enabled })
}

export function setSkillEnvVar(skill: string, key: string, value: string): Promise<void> {
  return getTransport().call("set_skill_env_var", { skill, key, value })
}

export function removeSkillEnvVar(skill: string, key: string): Promise<void> {
  return getTransport().call("remove_skill_env_var", { skill, key })
}

export function getSkillsEnvStatus(): Promise<Record<string, Record<string, boolean>>> {
  return getTransport().call("get_skills_env_status")
}

export function getSkillsStatus(): Promise<SkillStatusEntry[]> {
  return getTransport().call("get_skills_status")
}

export function getSkillEnv(name: string): Promise<Record<string, string>> {
  return getTransport().call("get_skill_env", { name })
}

export function installSkillDependency(skillName: string, specIndex: number): Promise<string> {
  return getTransport().call("install_skill_dependency", { skillName, specIndex })
}

export function loadSkillDockSnapshot(): Promise<SkillDockSnapshot> {
  return getTransport().call("get_skill_dock_snapshot")
}

export function loadSkillRegistrySnapshot(): Promise<SkillRegistrySnapshot> {
  return getTransport().call("get_skill_registry_snapshot")
}

export function loadDefaultSkillMarketSnapshot(): Promise<SkillRemoteMarketSnapshot> {
  return getTransport().call("get_default_skill_market_snapshot")
}

export function loadSkillMarketSnapshot(sourceUrls?: string[]): Promise<SkillRemoteMarketSnapshot> {
  return getTransport().call("get_skill_market_snapshot", { sourceUrls })
}

export function loadSkillMarketSources(): Promise<string[]> {
  return getTransport().call("get_skill_market_sources")
}

export function saveSkillMarketSources(sourceUrls: string[]): Promise<string[]> {
  return getTransport().call("set_skill_market_sources", { sourceUrls })
}

export function loadSkillMarketHubConfig(): Promise<SkillMarketHubConfigFile> {
  return getTransport().call("get_skill_market_hub_config")
}

export function upsertSkillMarketHub(request: SkillMarketHubUpsertRequest): Promise<SkillMarketHubConfigFile> {
  return getTransport().call("upsert_skill_market_hub", { request })
}

export function deleteSkillMarketHub(hubId: string): Promise<SkillMarketHubConfigFile> {
  return getTransport().call("delete_skill_market_hub", { hubId })
}

export function setSkillMarketHubEnabled(hubId: string, enabled: boolean): Promise<SkillMarketHubConfigFile> {
  return getTransport().call("set_skill_market_hub_enabled", { hubId, enabled })
}

export function loadSkillMarketHubTokenStatus(hubId: string): Promise<SkillMarketHubTokenStatus> {
  return getTransport().call("get_skill_market_hub_token_status", { hubId })
}

export function setSkillMarketHubToken(hubId: string, token: string): Promise<SkillMarketHubTokenStatus> {
  return getTransport().call("set_skill_market_hub_token", { hubId, token })
}

export function clearSkillMarketHubToken(hubId: string): Promise<SkillMarketHubTokenStatus> {
  return getTransport().call("clear_skill_market_hub_token", { hubId })
}

export function upsertSkillMarketRegistry(
  request: SkillMarketRegistryUpsertRequest,
): Promise<SkillMarketHubConfigFile> {
  return getTransport().call("upsert_skill_market_registry", { request })
}

export function deleteSkillMarketRegistry(registryId: string): Promise<SkillMarketHubConfigFile> {
  return getTransport().call("delete_skill_market_registry", { registryId })
}

export function createSkillPublishDraft(request: SkillPublishDraftRequest): Promise<SkillPublishDraft> {
  return getTransport().call("create_skill_publish_draft", { request })
}

export function pushSkillToMarketHub(request: SkillPublishPushRequest): Promise<SkillPublishPushResult> {
  return getTransport().call("push_skill_to_market_hub", { request })
}

export function marketInstallRequest(entry: SkillRemoteMarketEntry): SkillRemoteMarketInstallRequest {
  return {
    name: entry.name,
    source: entry.source,
    sourceType: entry.sourceType,
    skillPath: entry.skillPath,
    marketHash: entry.marketHash,
    marketVersion: entry.marketVersion,
    sourceId: entry.sourceId,
    sourceName: entry.sourceName,
  }
}

export function installRemoteMarketSkill(entry: SkillRemoteMarketEntry): Promise<SkillRemoteMarketInstallReport> {
  return getTransport().call("install_remote_market_skill", { request: marketInstallRequest(entry) })
}

export function updateRemoteMarketSkill(entry: SkillRemoteMarketEntry): Promise<SkillRemoteMarketInstallReport> {
  return getTransport().call("update_remote_market_skill", { request: marketInstallRequest(entry) })
}

export function installRegistrySkill(skillPath: string, name?: string): Promise<SkillRegistryInstallReport> {
  return getTransport().call("install_registry_skill", { skillPath, name })
}

export function updateRegistrySkill(skillPath: string, name?: string): Promise<SkillRegistryInstallReport> {
  return getTransport().call("update_registry_skill", { skillPath, name })
}

export function installSkillToApp(name: string, app: string): Promise<SkillAppInstallReport> {
  return getTransport().call("install_skill_to_app", { name, app })
}

export function uninstallSkillFromApp(name: string, app: string): Promise<SkillUninstallReport> {
  return getTransport().call("uninstall_skill_from_app", { name, app })
}

export function uninstallManagedSkill(name: string): Promise<SkillUninstallReport> {
  return getTransport().call("uninstall_managed_skill", { name })
}

export function scanSkillUsage(): Promise<SkillUsageScanReport> {
  return getTransport().call("scan_skill_usage")
}

export function dryRunImportSkillZip(path: string): Promise<SkillZipDryRunReport> {
  return getTransport().call("dry_run_import_skill_zip", { path })
}

export function importSkillZip(path: string): Promise<SkillZipImportReport> {
  return getTransport().call("import_skill_zip", { path })
}

export function importSkillZipRenamed(path: string): Promise<SkillZipImportReport> {
  return getTransport().call("import_skill_zip_renamed", { path })
}

export function exportSkillZip(name: string, outputPath: string): Promise<SkillZipExportReport> {
  return getTransport().call("export_skill_zip", { name, outputPath })
}
