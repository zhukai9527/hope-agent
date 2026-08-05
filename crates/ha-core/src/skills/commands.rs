//! Shared command-layer skill operations used by both the Tauri desktop
//! shell ([`src-tauri/src/commands/skills.rs`]) and the HTTP server
//! ([`crates/ha-server/src/routes/skills.rs`]).
//!
//! Each function owns its config read / mutation and is transport-agnostic:
//! callers only translate request extraction and response formatting. The
//! `source: &str` argument (typically `"settings-ui"` or `"http"`) tags the
//! autosave backup so users / operators can trace which surface triggered a
//! change.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use reqwest::redirect::Policy;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::security::ssrf::{check_url, SsrfPolicy};

use super::{
    author, auto_review, binary_in_path_public, bump_skill_version, check_all_skills_status,
    get_skill_content, is_masked_value, load_all_skills_with_budget, mask_value, SkillDetail,
    SkillStatus, SkillStatusEntry, SkillSummary,
};

const ZIP_DRY_RUN_MAX_ENTRIES: usize = 512;
const ZIP_DRY_RUN_MAX_UNCOMPRESSED_BYTES: u64 = 100 * 1024 * 1024;
const CLAWHUB_SKILLS_LOCK_URL: &str =
    "https://raw.githubusercontent.com/openclaw/clawhub/main/skills-lock.json";
const REMOTE_MARKET_MAX_BYTES: u64 = 512 * 1024;
const REMOTE_MARKET_MAX_ENTRIES: usize = 200;
const REMOTE_MARKET_MAX_SOURCES: usize = 5;
const REMOTE_SKILL_ARCHIVE_MAX_BYTES: u64 = 25 * 1024 * 1024;
const REMOTE_SKILL_EXTRACT_MAX_ENTRIES: usize = 1024;
const REMOTE_SKILL_EXTRACT_MAX_BYTES: u64 = 100 * 1024 * 1024;
const SKILL_MARKET_PUBLISH_RESPONSE_MAX_BYTES: u64 = 64 * 1024;
const GITHUB_RAW_HOST: &str = "raw.githubusercontent.com";
const GITHUB_MARKET_ALLOWED_HOSTS: &[&str] = &[GITHUB_RAW_HOST];

const DEFAULT_SKILL_MARKET_SOURCES: &[SkillRemoteMarketSourceSeed] =
    &[SkillRemoteMarketSourceSeed {
        id: "clawhub-official",
        name: "ClawHub Official",
        url: CLAWHUB_SKILLS_LOCK_URL,
        license: "MIT",
        source_type: "clawhub-lock",
    }];

struct SkillRemoteMarketSourceSeed {
    id: &'static str,
    name: &'static str,
    url: &'static str,
    license: &'static str,
    source_type: &'static str,
}

// ── Skills-Dock compatibility snapshot / package dry-run ─────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDockSnapshot {
    pub sources: Vec<SkillSourceRecord>,
    pub packages: Vec<SkillPackageSummary>,
    pub usage: Vec<SkillUsageSnapshot>,
    pub usage_trend: Vec<SkillUsageTrendPoint>,
    pub recent_usage: Vec<SkillUsageRecentRecord>,
    pub usage_app_breakdown: Vec<SkillUsageAppBreakdown>,
    pub apps: Vec<SkillAppProbe>,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSourceRecord {
    pub id: String,
    pub name: String,
    pub tool_kind: String,
    pub source_type: String,
    pub root_path: String,
    pub status: String,
    pub last_indexed_at: Option<String>,
    pub issues: Vec<SkillValidationIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillValidationIssue {
    pub code: String,
    pub message: String,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillAppInstallState {
    pub app: String,
    pub installed: bool,
    pub state: String,
    pub target_path: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillUsageSnapshot {
    pub skill_name: String,
    pub usage_count: u64,
    pub last_used_at: Option<String>,
    pub apps: Vec<SkillAppInstallState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillUsageTrendPoint {
    pub date: String,
    pub app: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillUsageRecentRecord {
    pub activated_at: String,
    pub app: String,
    pub skill_name: String,
    pub session_id: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillUsageAppBreakdown {
    pub app: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillPackageSummary {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub channel: String,
    pub source_status: String,
    pub install_state: String,
    pub description: Option<String>,
    pub read_only: bool,
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRegistrySnapshot {
    pub entries: Vec<SkillRegistryEntry>,
    pub sources: Vec<SkillSourceRecord>,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRegistryEntry {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub source_id: String,
    pub source_path: String,
    pub skill_path: String,
    pub category: String,
    pub tags: Vec<String>,
    pub version: Option<String>,
    pub updated_at: Option<String>,
    pub installed: bool,
    pub update_available: bool,
    pub installed_state: String,
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRemoteMarketSnapshot {
    pub sources: Vec<SkillRemoteMarketSource>,
    pub entries: Vec<SkillRemoteMarketEntry>,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRemoteMarketSource {
    pub id: String,
    pub name: String,
    pub url: String,
    pub license: String,
    pub read_only: bool,
    pub source_type: String,
    pub status: String,
    pub error: Option<String>,
    pub entry_count: usize,
    pub category_counts: HashMap<String, usize>,
    pub installed_count: usize,
    pub update_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillMarketHubConfigFile {
    pub default_hub_id: String,
    pub hubs: Vec<SkillMarketHubConfig>,
    pub registries: Vec<SkillMarketRegistryConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillMarketHubConfig {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub kind: String,
    pub source_type: String,
    pub token_ref: Option<String>,
    pub read_only: bool,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillMarketRegistryConfig {
    pub id: String,
    pub hub_id: String,
    pub name: String,
    pub registry_url: String,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillMarketHubTokenStatus {
    pub hub_id: String,
    pub token_ref: Option<String>,
    pub has_token: bool,
    pub masked: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillMarketHubTokenCredential {
    token: String,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillMarketHubUpsertRequest {
    pub id: Option<String>,
    pub name: String,
    pub base_url: String,
    pub kind: String,
    pub source_type: String,
    pub read_only: Option<bool>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillMarketRegistryUpsertRequest {
    pub id: Option<String>,
    pub hub_id: String,
    pub name: String,
    pub registry_url: String,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillPublishDraftRequest {
    pub skill_name: String,
    pub hub_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillPublishDraft {
    pub ok: bool,
    pub status: String,
    pub error: Option<String>,
    pub skill_name: String,
    pub hub_id: String,
    pub source_id: String,
    pub registry_url: String,
    pub manifest: serde_json::Value,
    pub readme: String,
    pub hash: String,
    pub publishable: bool,
    pub token_required: bool,
    pub token_configured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillPublishPushRequest {
    pub skill_name: String,
    pub hub_id: String,
    pub confirmed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillPublishPushResult {
    pub ok: bool,
    pub status: String,
    pub error: Option<String>,
    pub skill_name: String,
    pub source_id: String,
    pub registry_url: String,
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRemoteMarketEntry {
    pub id: String,
    pub source_id: String,
    pub source_name: String,
    pub name: String,
    pub source: String,
    pub source_type: String,
    pub skill_path: String,
    pub raw_url: String,
    pub description: String,
    pub author: String,
    pub license: String,
    pub category: String,
    pub tags: Vec<String>,
    pub rating: f32,
    pub download_count: u64,
    pub updated_at: Option<String>,
    pub featured: bool,
    pub compatible_apps: Vec<String>,
    pub market_version: Option<String>,
    pub installed_version: Option<String>,
    pub market_hash: Option<String>,
    pub installed_hash: Option<String>,
    pub comparison_basis: String,
    pub installed: bool,
    pub update_available: bool,
    pub update_reason: Option<String>,
    pub installed_state: String,
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ClawHubSkillsLock {
    skills: HashMap<String, ClawHubSkillLockEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClawHubSkillLockEntry {
    source: String,
    source_type: String,
    skill_path: String,
    computed_hash: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct SkillRemoteMarketMetadata {
    name: Option<String>,
    description: Option<String>,
    author: Option<String>,
    license: Option<String>,
    version: Option<String>,
    category: Option<String>,
    tags: Vec<String>,
    updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRegistryInstallReport {
    pub name: String,
    pub source_path: String,
    pub target_path: String,
    pub installed: bool,
    pub updated: bool,
    pub backup_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRemoteMarketInstallRequest {
    pub name: String,
    pub source: String,
    pub source_type: String,
    pub skill_path: String,
    pub market_hash: Option<String>,
    pub market_version: Option<String>,
    pub source_id: Option<String>,
    pub source_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRemoteMarketInstallReport {
    pub name: String,
    pub source: String,
    pub skill_path: String,
    pub target_path: String,
    pub installed: bool,
    pub updated: bool,
    pub verified_hash: Option<String>,
    pub backup_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillRemoteMarketInstalledMetadata {
    name: String,
    source: String,
    source_type: String,
    skill_path: String,
    source_id: Option<String>,
    source_name: Option<String>,
    market_hash: Option<String>,
    market_version: Option<String>,
    installed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillAppInstallReport {
    pub skill_name: String,
    pub app: String,
    pub source_path: String,
    pub target_path: String,
    pub installed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillUninstallReport {
    pub skill_name: String,
    pub removed_path: String,
    pub removed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillUsageScanReport {
    pub usage: Vec<SkillUsageSnapshot>,
    pub usage_trend: Vec<SkillUsageTrendPoint>,
    pub recent_usage: Vec<SkillUsageRecentRecord>,
    pub usage_app_breakdown: Vec<SkillUsageAppBreakdown>,
    pub scanned_at: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillAppProbe {
    pub app: String,
    pub installed: bool,
    pub root_path: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillZipDryRunRequest {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillZipDryRunReport {
    pub ok: bool,
    pub path: String,
    pub entry_count: usize,
    pub total_uncompressed_size: u64,
    pub skill_count: usize,
    pub skill_names: Vec<String>,
    pub entries: Vec<SkillZipEntryPreview>,
    pub issues: Vec<SkillValidationIssue>,
    pub dry_run_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillZipEntryPreview {
    pub name: String,
    pub size: u64,
    pub compressed_size: u64,
    pub is_dir: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillZipImportReport {
    pub imported: Vec<String>,
    pub renamed: HashMap<String, String>,
    pub target_dir: String,
    pub dry_run: SkillZipDryRunReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillZipExportReport {
    pub skill_name: String,
    pub output_path: String,
    pub entry_count: usize,
    pub total_uncompressed_size: u64,
}

// ── Helper functions for Skills-Dock snapshot / ZIP dry-run ────────

fn probe_skill_apps() -> Vec<SkillAppProbe> {
    let mut probes = Vec::new();
    let hope_root = crate::paths::skills_dir().ok();
    probes.push(app_probe("hope", hope_root));
    if let Some(home) = dirs::home_dir() {
        probes.push(app_probe(
            "claude",
            Some(home.join(".claude").join("skills")),
        ));
        probes.push(app_probe("codex", Some(home.join(".codex").join("skills"))));
        probes.push(app_probe(
            "gemini",
            Some(home.join(".gemini").join("skills")),
        ));
        probes.push(app_probe(
            "opencode",
            Some(home.join(".opencode").join("skills")),
        ));
    }
    probes
}

fn app_probe(app: &str, root: Option<PathBuf>) -> SkillAppProbe {
    let installed = root.as_ref().map(|p| p.is_dir()).unwrap_or(false);
    SkillAppProbe {
        app: app.to_string(),
        installed,
        root_path: root.map(|p| p.to_string_lossy().to_string()),
        status: if installed { "ready" } else { "missing" }.to_string(),
    }
}

fn build_skill_source_records(
    extra_dirs: &[String],
    skills: &[SkillSummary],
    _now: &str,
) -> Vec<SkillSourceRecord> {
    let mut records = Vec::new();
    let mut seen_roots = std::collections::HashSet::new();
    for skill in skills {
        let root = skill_source_root(skill);
        if !seen_roots.insert(root.clone()) {
            continue;
        }
        let status = if Path::new(&root).is_dir() {
            "ready"
        } else {
            "missing"
        };
        records.push(SkillSourceRecord {
            id: format!("{}:{}", skill.source, root),
            name: skill.source.clone(),
            tool_kind: "hope".to_string(),
            source_type: source_type_for_source(&skill.source).to_string(),
            root_path: root,
            status: status.to_string(),
            last_indexed_at: None,
            issues: Vec::new(),
        });
    }
    // Include extra dirs that may not have any loaded skills yet.
    for dir in extra_dirs {
        if !seen_roots.contains(dir) {
            let status = if Path::new(dir).is_dir() {
                "ready"
            } else {
                "missing"
            };
            records.push(SkillSourceRecord {
                id: dir.clone(),
                name: dir.clone(),
                tool_kind: "hope".to_string(),
                source_type: "custom".to_string(),
                root_path: dir.clone(),
                status: status.to_string(),
                last_indexed_at: None,
                issues: if status == "missing" {
                    vec![SkillValidationIssue {
                        code: "source_missing".to_string(),
                        message: format!("Configured skills directory does not exist: {dir}"),
                        severity: "warning".to_string(),
                    }]
                } else {
                    Vec::new()
                },
            });
        }
    }
    records
}

fn skill_source_root(skill: &SkillSummary) -> String {
    let base = Path::new(&skill.base_dir);
    match base.parent() {
        Some(parent) => parent.to_string_lossy().to_string(),
        None => skill.base_dir.clone(),
    }
}

fn source_type_for_source(source: &str) -> &str {
    match source {
        "bundled" => "bundled",
        "managed" | "project" | "shared" => "user",
        _ => "custom",
    }
}

fn package_channel_for_source(source: &str) -> &str {
    if source.starts_with("http") {
        "registry"
    } else {
        "local"
    }
}

fn app_install_states_for_skill(
    skill: &SkillSummary,
    apps: &[SkillAppProbe],
) -> Vec<SkillAppInstallState> {
    apps.iter()
        .map(|app| {
            let external_skill_dir = app
                .root_path
                .as_ref()
                .map(|root| Path::new(root).join(&skill.name));
            let external_installed = external_skill_dir
                .as_ref()
                .map(|dir| dir.join("SKILL.md").is_file())
                .unwrap_or(false);
            let installed = if app.app == "hope" {
                skill.enabled
            } else {
                external_installed
            };
            SkillAppInstallState {
                app: app.app.clone(),
                installed,
                state: if app.app == "hope" && skill.enabled {
                    "ready".to_string()
                } else if external_installed {
                    "external".to_string()
                } else if app.installed {
                    "attention".to_string()
                } else {
                    "external".to_string()
                },
                target_path: external_skill_dir.map(|p| p.to_string_lossy().to_string()),
                reason: if app.app == "hope" {
                    Some("Managed by Hope Agent skill catalog.".to_string())
                } else if external_installed {
                    Some(
                        "Matching SKILL.md found in the external app skills directory.".to_string(),
                    )
                } else if app.installed {
                    Some(
                        "App skills directory exists, but this skill is not installed there."
                            .to_string(),
                    )
                } else {
                    Some("External app skills directory was not found.".to_string())
                },
            }
        })
        .collect()
}

fn zip_issue(code: &str, message: String, severity: &str) -> SkillValidationIssue {
    SkillValidationIssue {
        code: code.to_string(),
        message,
        severity: severity.to_string(),
    }
}

fn validate_zip_entry_name(name: &str, issues: &mut Vec<SkillValidationIssue>) {
    if name.is_empty() {
        issues.push(zip_issue(
            "empty_entry_name",
            "ZIP entry has an empty name.".to_string(),
            "warning",
        ));
        return;
    }
    let parts = name.split('/');
    if parts.clone().any(|part| part == "..") || name.contains("../") || name.contains("/..") {
        issues.push(zip_issue(
            "path_traversal",
            format!("ZIP entry '{}' contains path traversal ('..').", name),
            "error",
        ));
    }
    if name.starts_with('/') || name.starts_with('\\') {
        issues.push(zip_issue(
            "absolute_path",
            format!("ZIP entry '{}' is an absolute path.", name),
            "error",
        ));
    }
    if name
        .split('/')
        .any(|part| part.contains('\0') || part.contains(':'))
    {
        issues.push(zip_issue(
            "unsafe_entry_name",
            format!("ZIP entry '{}' contains an unsafe path segment.", name),
            "error",
        ));
    }
}

fn infer_skill_name_from_zip_entry(entry: &str) -> Option<String> {
    // Expected layout: `<skill-name>/SKILL.md` or `skills/<skill-name>/SKILL.md`.
    let normalized = entry.trim_end_matches('/');
    if !normalized.ends_with("SKILL.md") {
        return None;
    }
    let parent = normalized.strip_suffix("SKILL.md")?.trim_end_matches('/');
    let name = parent.rsplit('/').next().unwrap_or(parent);
    if name.is_empty() || name == "skills" {
        return None;
    }
    Some(name.to_string())
}

fn normalize_skill_zip_relative_path(entry: &str) -> Option<(String, PathBuf)> {
    let normalized = entry.trim_matches('/').replace('\\', "/");
    if normalized.is_empty() {
        return None;
    }
    let mut parts: Vec<&str> = normalized
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    if parts.first().copied() == Some("skills") {
        parts.remove(0);
    }
    let skill_name = parts.first()?.to_string();
    if skill_name == "." || skill_name == ".." {
        return None;
    }
    let mut relative = PathBuf::new();
    for part in parts.iter().skip(1) {
        relative.push(part);
    }
    Some((skill_name, relative))
}

fn has_zip_error(report: &SkillZipDryRunReport) -> bool {
    report.issues.iter().any(|issue| issue.severity == "error")
}

pub fn get_skill_dock_snapshot() -> SkillDockSnapshot {
    let store = crate::config::cached_config();
    let skills = list_skills();
    let now = chrono::Utc::now().to_rfc3339();
    let apps = probe_skill_apps();
    let sources = build_skill_source_records(&store.extra_skills_dirs, &skills, &now);
    let packages = skills
        .iter()
        .map(|skill| SkillPackageSummary {
            id: skill.name.clone(),
            name: skill.name.clone(),
            version: skill.display.version.clone(),
            channel: package_channel_for_source(&skill.source).to_string(),
            source_status: "ready".to_string(),
            install_state: if skill.enabled { "ready" } else { "attention" }.to_string(),
            description: Some(skill.description.clone()),
            read_only: true,
            actions: vec![
                "preview".to_string(),
                "dryRunImport".to_string(),
                "export".to_string(),
            ],
        })
        .collect();
    let usage = skills
        .iter()
        .map(|skill| SkillUsageSnapshot {
            skill_name: skill.name.clone(),
            usage_count: 0,
            last_used_at: None,
            apps: app_install_states_for_skill(skill, &apps),
        })
        .collect();

    SkillDockSnapshot {
        sources,
        packages,
        usage,
        usage_trend: vec![],
        recent_usage: vec![],
        usage_app_breakdown: vec![],
        apps,
        generated_at: now,
    }
}

pub fn get_skill_dock_snapshot_with_usage(
    db: &crate::session::SessionDB,
) -> Result<SkillDockSnapshot> {
    let mut snapshot = get_skill_dock_snapshot();
    let usage_report = scan_skill_usage(db)?;
    let mut usage_by_name: HashMap<String, SkillUsageSnapshot> = usage_report
        .usage
        .into_iter()
        .map(|row| (row.skill_name.clone(), row))
        .collect();
    for row in &mut snapshot.usage {
        if let Some(usage) = usage_by_name.remove(&row.skill_name) {
            row.usage_count = usage.usage_count;
            row.last_used_at = usage.last_used_at;
        }
    }
    snapshot.usage.extend(usage_by_name.into_values());
    Ok(snapshot)
}

pub fn dry_run_import_skill_zip(path: String) -> Result<SkillZipDryRunReport> {
    let input = PathBuf::from(&path);
    let canonical = input
        .canonicalize()
        .with_context(|| format!("Cannot access ZIP archive: {}", input.display()))?;
    if !canonical.is_file() {
        return Err(anyhow!(
            "ZIP import dry-run expects a file: {}",
            canonical.display()
        ));
    }
    let file = File::open(&canonical)
        .with_context(|| format!("Cannot open ZIP archive: {}", canonical.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("Invalid ZIP archive: {}", canonical.display()))?;

    let mut entries = Vec::new();
    let mut issues = Vec::new();
    let mut total_uncompressed_size = 0_u64;
    let mut skill_names = Vec::new();

    if archive.len() > ZIP_DRY_RUN_MAX_ENTRIES {
        issues.push(zip_issue(
            "too_many_entries",
            format!(
                "Archive has {} entries; dry-run limit is {}.",
                archive.len(),
                ZIP_DRY_RUN_MAX_ENTRIES
            ),
            "error",
        ));
    }

    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .with_context(|| format!("Cannot read ZIP entry #{index}"))?;
        let raw_name = entry.name().to_string();
        let normalized_name = raw_name.replace('\\', "/");
        total_uncompressed_size = total_uncompressed_size.saturating_add(entry.size());

        validate_zip_entry_name(&normalized_name, &mut issues);
        if entry
            .unix_mode()
            .map(|mode| mode & 0o170000 == 0o120000)
            .unwrap_or(false)
        {
            issues.push(zip_issue(
                "symlink_entry",
                format!(
                    "ZIP entry '{}' looks like a symbolic link.",
                    normalized_name
                ),
                "error",
            ));
        }
        if normalized_name.ends_with("SKILL.md") {
            if let Some(name) = infer_skill_name_from_zip_entry(&normalized_name) {
                if !skill_names.contains(&name) {
                    skill_names.push(name);
                }
            }
        }

        if entries.len() < 80 {
            entries.push(SkillZipEntryPreview {
                name: normalized_name,
                size: entry.size(),
                compressed_size: entry.compressed_size(),
                is_dir: entry.is_dir(),
            });
        }
    }

    if total_uncompressed_size > ZIP_DRY_RUN_MAX_UNCOMPRESSED_BYTES {
        issues.push(zip_issue(
            "archive_too_large",
            format!(
                "Archive expands to {} bytes; dry-run limit is {} bytes.",
                total_uncompressed_size, ZIP_DRY_RUN_MAX_UNCOMPRESSED_BYTES
            ),
            "error",
        ));
    }

    if skill_names.is_empty() {
        issues.push(zip_issue(
            "no_skill_manifest",
            "No SKILL.md entries were found in this ZIP.".to_string(),
            "warning",
        ));
    }
    skill_names.sort();

    let ok = !issues.iter().any(|issue| issue.severity == "error");
    Ok(SkillZipDryRunReport {
        ok,
        path: canonical.to_string_lossy().to_string(),
        entry_count: archive.len(),
        total_uncompressed_size,
        skill_count: skill_names.len(),
        skill_names,
        entries,
        issues,
        dry_run_only: true,
    })
}

pub fn import_skill_zip(path: String) -> Result<SkillZipImportReport> {
    import_skill_zip_with_strategy(path, false)
}

pub fn import_skill_zip_renamed(path: String) -> Result<SkillZipImportReport> {
    import_skill_zip_with_strategy(path, true)
}

fn import_skill_zip_with_strategy(
    path: String,
    rename_conflicts: bool,
) -> Result<SkillZipImportReport> {
    let dry_run = dry_run_import_skill_zip(path.clone())?;
    if has_zip_error(&dry_run) || dry_run.skill_names.is_empty() {
        return Err(anyhow!(
            "ZIP import dry-run failed; refusing to extract archive."
        ));
    }

    let target_root =
        crate::paths::skills_dir().context("Cannot resolve Hope Agent managed skills directory")?;
    let mut target_names = HashMap::new();
    for skill_name in &dry_run.skill_names {
        let target_name = if rename_conflicts {
            allocate_available_skill_name(&target_root, skill_name)?
        } else {
            skill_name.clone()
        };
        let target = target_root.join(&target_name);
        if target.exists() {
            return Err(anyhow!(
                "Skill '{}' already exists at {}; refusing to overwrite.",
                skill_name,
                target.display()
            ));
        }
        target_names.insert(skill_name.clone(), target_name);
    }

    fs::create_dir_all(&target_root)
        .with_context(|| format!("Cannot create skills directory: {}", target_root.display()))?;
    let temp_dir = tempfile::tempdir_in(&target_root).with_context(|| {
        format!(
            "Cannot create staging directory in {}",
            target_root.display()
        )
    })?;
    let input = PathBuf::from(&dry_run.path);
    let file = File::open(&input)
        .with_context(|| format!("Cannot open ZIP archive: {}", input.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("Invalid ZIP archive: {}", input.display()))?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .with_context(|| format!("Cannot read ZIP entry #{index}"))?;
        let normalized_name = entry.name().replace('\\', "/");
        let Some((skill_name, relative)) = normalize_skill_zip_relative_path(&normalized_name)
        else {
            continue;
        };
        if !dry_run.skill_names.contains(&skill_name) || relative.as_os_str().is_empty() {
            continue;
        }

        let target_name = target_names
            .get(&skill_name)
            .ok_or_else(|| anyhow!("Missing target name for ZIP skill '{skill_name}'"))?;
        let out_path = temp_dir.path().join(target_name).join(&relative);
        if entry.is_dir() {
            fs::create_dir_all(&out_path)
                .with_context(|| format!("Cannot create directory: {}", out_path.display()))?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Cannot create directory: {}", parent.display()))?;
        }
        let mut out = File::create(&out_path)
            .with_context(|| format!("Cannot create file: {}", out_path.display()))?;
        std::io::copy(&mut entry, &mut out)
            .with_context(|| format!("Cannot extract ZIP entry: {}", normalized_name))?;
    }

    for skill_name in &dry_run.skill_names {
        let target_name = target_names
            .get(skill_name)
            .ok_or_else(|| anyhow!("Missing target name for ZIP skill '{skill_name}'"))?;
        let staged = temp_dir.path().join(target_name);
        if !staged.join("SKILL.md").is_file() {
            return Err(anyhow!(
                "Extracted skill '{}' is missing SKILL.md.",
                skill_name
            ));
        }
    }

    let mut imported = Vec::new();
    let mut renamed = HashMap::new();
    for skill_name in &dry_run.skill_names {
        let target_name = target_names
            .get(skill_name)
            .ok_or_else(|| anyhow!("Missing target name for ZIP skill '{skill_name}'"))?;
        let staged = temp_dir.path().join(target_name);
        let target = target_root.join(target_name);
        if target.exists() {
            return Err(anyhow!(
                "Skill '{}' appeared during import at {}; refusing to overwrite.",
                skill_name,
                target.display()
            ));
        }
        fs::rename(&staged, &target).with_context(|| {
            format!(
                "Cannot move imported skill '{}' from {} to {}",
                skill_name,
                staged.display(),
                target.display()
            )
        })?;
        imported.push(target_name.clone());
        if target_name != skill_name {
            renamed.insert(skill_name.clone(), target_name.clone());
        }
    }
    bump_skill_version();

    Ok(SkillZipImportReport {
        imported,
        renamed,
        target_dir: target_root.to_string_lossy().to_string(),
        dry_run,
    })
}

fn allocate_available_skill_name(target_root: &Path, skill_name: &str) -> Result<String> {
    validate_skill_dir_name(skill_name)?;
    if !target_root.join(skill_name).exists() {
        return Ok(skill_name.to_string());
    }
    for index in 1..=100 {
        let candidate = if index == 1 {
            format!("{skill_name}-copy")
        } else {
            format!("{skill_name}-copy-{index}")
        };
        validate_skill_dir_name(&candidate)?;
        if !target_root.join(&candidate).exists() {
            return Ok(candidate);
        }
    }
    Err(anyhow!(
        "Cannot allocate a non-conflicting skill name for '{}'.",
        skill_name
    ))
}

pub fn export_skill_zip(name: String, output_path: String) -> Result<SkillZipExportReport> {
    let detail = get_skill_detail(&name).ok_or_else(|| anyhow!("Skill not found: {}", name))?;
    let source_root = PathBuf::from(&detail.base_dir)
        .canonicalize()
        .with_context(|| format!("Cannot access skill directory: {}", detail.base_dir))?;
    if !source_root.join("SKILL.md").is_file() {
        return Err(anyhow!(
            "Skill '{}' does not have a SKILL.md file.",
            detail.name
        ));
    }

    let requested = PathBuf::from(output_path);
    let target = if requested.is_dir() {
        requested.join(format!("{}.zip", detail.name))
    } else {
        requested
    };
    if target.exists() {
        return Err(anyhow!(
            "Export target already exists at {}; refusing to overwrite.",
            target.display()
        ));
    }
    let parent = target
        .parent()
        .ok_or_else(|| anyhow!("Export target must have a parent directory."))?;
    if !parent.is_dir() {
        return Err(anyhow!(
            "Export target parent directory does not exist: {}",
            parent.display()
        ));
    }

    let file = File::create(&target)
        .with_context(|| format!("Cannot create ZIP export: {}", target.display()))?;
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    let mut entry_count = 0_usize;
    let mut total_uncompressed_size = 0_u64;
    add_skill_dir_to_zip(
        &mut writer,
        &source_root,
        &source_root,
        &detail.name,
        options,
        &mut entry_count,
        &mut total_uncompressed_size,
    )?;
    writer
        .finish()
        .with_context(|| format!("Cannot finish ZIP export: {}", target.display()))?;

    Ok(SkillZipExportReport {
        skill_name: detail.name,
        output_path: target.to_string_lossy().to_string(),
        entry_count,
        total_uncompressed_size,
    })
}

fn add_skill_dir_to_zip(
    writer: &mut zip::ZipWriter<File>,
    root: &Path,
    dir: &Path,
    skill_name: &str,
    options: zip::write::SimpleFileOptions,
    entry_count: &mut usize,
    total_uncompressed_size: &mut u64,
) -> Result<()> {
    for entry in
        fs::read_dir(dir).with_context(|| format!("Cannot read directory: {}", dir.display()))?
    {
        let entry =
            entry.with_context(|| format!("Cannot read directory entry in {}", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("Cannot inspect file type: {}", path.display()))?;
        if file_type.is_symlink() {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .with_context(|| format!("Cannot compute relative path for {}", path.display()))?;
        let zip_name = format!(
            "{}/{}",
            skill_name,
            rel.to_string_lossy().replace('\\', "/")
        );
        if file_type.is_dir() {
            writer
                .add_directory(format!("{}/", zip_name.trim_end_matches('/')), options)
                .with_context(|| format!("Cannot add ZIP directory: {zip_name}"))?;
            *entry_count += 1;
            add_skill_dir_to_zip(
                writer,
                root,
                &path,
                skill_name,
                options,
                entry_count,
                total_uncompressed_size,
            )?;
        } else if file_type.is_file() {
            let metadata = entry
                .metadata()
                .with_context(|| format!("Cannot stat file: {}", path.display()))?;
            *total_uncompressed_size = total_uncompressed_size.saturating_add(metadata.len());
            writer
                .start_file(zip_name.clone(), options)
                .with_context(|| format!("Cannot add ZIP file: {zip_name}"))?;
            let mut input = File::open(&path)
                .with_context(|| format!("Cannot open file for ZIP export: {}", path.display()))?;
            std::io::copy(&mut input, writer)
                .with_context(|| format!("Cannot write ZIP file entry: {zip_name}"))?;
            *entry_count += 1;
        }
    }
    Ok(())
}

// ── Registry / marketplace + cross-app install management ───────────

pub fn get_skill_registry_snapshot() -> SkillRegistrySnapshot {
    let store = crate::config::cached_config();
    let installed_skills = list_skills();
    let installed_by_name: HashMap<String, SkillSummary> = installed_skills
        .iter()
        .cloned()
        .map(|skill| (skill.name.clone(), skill))
        .collect();
    let now = chrono::Utc::now().to_rfc3339();
    let sources = build_skill_source_records(&store.extra_skills_dirs, &installed_skills, &now);
    let mut entries = Vec::new();
    let mut seen = HashSet::new();

    for source in discover_preset_skill_sources() {
        for candidate in source.candidates.iter().filter(|c| c.exists) {
            let root = PathBuf::from(&candidate.path);
            for skill_dir in find_skill_dirs_limited(&root, 2) {
                let Some(name) = skill_dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_string())
                else {
                    continue;
                };
                let id = format!("{}:{}", source.id, skill_dir.to_string_lossy());
                if !seen.insert(id.clone()) {
                    continue;
                }
                let installed_skill = installed_by_name.get(&name);
                let installed = installed_skill.is_some();
                let update_available = installed_skill
                    .map(|skill| skill_content_differs(&skill_dir, Path::new(&skill.base_dir)))
                    .unwrap_or(false);
                entries.push(SkillRegistryEntry {
                    id,
                    name: name.clone(),
                    description: read_skill_description(&skill_dir),
                    source_id: source.id.clone(),
                    source_path: candidate.path.clone(),
                    skill_path: skill_dir.to_string_lossy().to_string(),
                    category: skill_market_category(&name),
                    tags: skill_market_tags(&name),
                    version: read_skill_display_version(&skill_dir),
                    updated_at: skill_dir_updated_at(&skill_dir),
                    installed,
                    update_available,
                    installed_state: if update_available {
                        "updateAvailable"
                    } else if installed {
                        "installed"
                    } else {
                        "available"
                    }
                    .to_string(),
                    actions: if installed {
                        let mut actions = vec!["preview".to_string(), "export".to_string()];
                        if update_available {
                            actions.push("update".to_string());
                        }
                        actions
                    } else {
                        vec!["preview".to_string(), "install".to_string()]
                    },
                });
            }
        }
    }

    entries.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.source_id.cmp(&b.source_id))
    });
    SkillRegistrySnapshot {
        entries,
        sources,
        generated_at: now,
    }
}

pub async fn get_default_skill_market_snapshot() -> Result<SkillRemoteMarketSnapshot> {
    get_skill_market_snapshot(None).await
}

pub async fn get_skill_market_snapshot(
    source_urls: Option<Vec<String>>,
) -> Result<SkillRemoteMarketSnapshot> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .redirect(Policy::none())
        .user_agent("HopeAgent/skill-market")
        .build()
        .context("Cannot create skill market HTTP client")?;
    let installed_by_name: HashMap<String, SkillSummary> = list_skills()
        .into_iter()
        .map(|skill| (skill.name.clone(), skill))
        .collect();

    let mut sources = Vec::new();
    let mut entries = Vec::new();

    let hub_config =
        read_skill_market_hub_config().unwrap_or_else(|_| default_skill_market_hub_config());
    let mut snapshot_source_urls = HashSet::new();

    for hub in hub_config.hubs.iter().filter(|hub| hub.enabled) {
        let mut source_record = SkillRemoteMarketSource {
            id: hub.id.clone(),
            name: hub.name.clone(),
            url: hub.base_url.clone(),
            license: if hub.id == DEFAULT_SKILL_MARKET_SOURCES[0].id {
                DEFAULT_SKILL_MARKET_SOURCES[0].license.to_string()
            } else {
                "unknown".to_string()
            },
            read_only: hub.read_only,
            source_type: hub.source_type.clone(),
            status: "ready".to_string(),
            error: None,
            entry_count: 0,
            category_counts: HashMap::new(),
            installed_count: 0,
            update_count: 0,
        };

        snapshot_source_urls.insert(hub.base_url.clone());
        match hub.source_type.as_str() {
            "clawhub-lock" => {
                match fetch_clawhub_market_entries(
                    &client,
                    &hub.id,
                    &hub.name,
                    &hub.base_url,
                    &installed_by_name,
                )
                .await
                {
                    Ok(mut fetched) => {
                        update_skill_market_source_stats(&mut source_record, &fetched);
                        entries.append(&mut fetched);
                    }
                    Err(error) => {
                        source_record.status = "error".to_string();
                        source_record.error = Some(error.to_string());
                    }
                }
            }
            _ => {
                source_record.status = "error".to_string();
                source_record.error = Some(format!(
                    "Skill market source type '{}' is configured for publish/registry management and is not a readable lock index.",
                    hub.source_type
                ));
            }
        }
        sources.push(source_record);
    }

    let configured_urls = source_urls.unwrap_or_else(get_skill_market_sources);
    if !configured_urls.is_empty() {
        for (index, url) in configured_urls
            .into_iter()
            .take(REMOTE_MARKET_MAX_SOURCES)
            .enumerate()
        {
            let trimmed = url.trim().to_string();
            if trimmed.is_empty() || snapshot_source_urls.contains(&trimmed) {
                continue;
            }
            let id = format!("custom-{}", index + 1);
            let name = reqwest::Url::parse(&trimmed)
                .ok()
                .and_then(|url| url.host_str().map(|host| host.to_string()))
                .unwrap_or_else(|| format!("Custom Market {}", index + 1));
            let mut source_record = SkillRemoteMarketSource {
                id: id.clone(),
                name: name.clone(),
                url: trimmed.clone(),
                license: "unknown".to_string(),
                read_only: true,
                source_type: "clawhub-lock".to_string(),
                status: "ready".to_string(),
                error: None,
                entry_count: 0,
                category_counts: HashMap::new(),
                installed_count: 0,
                update_count: 0,
            };
            match fetch_clawhub_market_entries(&client, &id, &name, &trimmed, &installed_by_name)
                .await
            {
                Ok(mut fetched) => {
                    update_skill_market_source_stats(&mut source_record, &fetched);
                    entries.append(&mut fetched);
                }
                Err(error) => {
                    source_record.status = "error".to_string();
                    source_record.error = Some(error.to_string());
                }
            }
            sources.push(source_record);
        }
    }

    entries.sort_by(|a, b| {
        a.source_name
            .cmp(&b.source_name)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.source.cmp(&b.source))
    });
    Ok(SkillRemoteMarketSnapshot {
        sources,
        entries,
        generated_at: chrono::Utc::now().to_rfc3339(),
    })
}

pub fn get_skill_market_sources() -> Vec<String> {
    read_skill_market_sources().unwrap_or_default()
}

pub fn set_skill_market_sources(source_urls: Vec<String>, source: &str) -> Result<Vec<String>> {
    let normalized = normalize_skill_market_sources(source_urls)?;
    write_skill_market_sources(&normalized, source)?;
    Ok(normalized)
}

pub fn get_skill_market_hub_config() -> Result<SkillMarketHubConfigFile> {
    read_skill_market_hub_config()
}

pub fn get_skill_market_hub_token_status(hub_id: String) -> Result<SkillMarketHubTokenStatus> {
    let hub_id = normalize_hub_id(&hub_id)?;
    let config = read_skill_market_hub_config()?;
    let hub = config
        .hubs
        .iter()
        .find(|hub| hub.id == hub_id)
        .ok_or_else(|| anyhow!("Skill market hub not found: {hub_id}"))?;
    let token_ref = hub
        .token_ref
        .clone()
        .unwrap_or_else(|| skill_market_hub_token_ref(&hub.id));
    let token = read_skill_market_hub_token_by_ref(&token_ref)?;
    Ok(SkillMarketHubTokenStatus {
        hub_id: hub.id.clone(),
        token_ref: Some(token_ref),
        has_token: token.is_some(),
        masked: token.as_deref().map(mask_skill_market_token),
    })
}

pub fn set_skill_market_hub_token(
    hub_id: String,
    token: String,
    source: &str,
) -> Result<SkillMarketHubTokenStatus> {
    let hub_id = normalize_hub_id(&hub_id)?;
    let token = token.trim();
    if token.is_empty() || is_masked_value(token) {
        return Err(anyhow!("Skill market hub token cannot be empty or masked."));
    }
    let mut config = read_skill_market_hub_config()?;
    let hub = config
        .hubs
        .iter_mut()
        .find(|hub| hub.id == hub_id)
        .ok_or_else(|| anyhow!("Skill market hub not found: {hub_id}"))?;
    if hub.read_only {
        return Err(anyhow!(
            "Read-only skill market hub cannot store a publish token."
        ));
    }
    let token_ref = hub
        .token_ref
        .clone()
        .unwrap_or_else(|| skill_market_hub_token_ref(&hub.id));
    write_skill_market_hub_token_by_ref(&token_ref, token)?;
    hub.token_ref = Some(token_ref.clone());
    hub.updated_at = chrono::Utc::now().to_rfc3339();
    write_skill_market_hub_config(&config, source)?;
    get_skill_market_hub_token_status(hub_id)
}

pub fn clear_skill_market_hub_token(
    hub_id: String,
    source: &str,
) -> Result<SkillMarketHubTokenStatus> {
    let hub_id = normalize_hub_id(&hub_id)?;
    let mut config = read_skill_market_hub_config()?;
    let hub = config
        .hubs
        .iter_mut()
        .find(|hub| hub.id == hub_id)
        .ok_or_else(|| anyhow!("Skill market hub not found: {hub_id}"))?;
    let token_ref = hub
        .token_ref
        .clone()
        .unwrap_or_else(|| skill_market_hub_token_ref(&hub.id));
    remove_skill_market_hub_token_by_ref(&token_ref)?;
    hub.token_ref = Some(token_ref);
    hub.updated_at = chrono::Utc::now().to_rfc3339();
    write_skill_market_hub_config(&config, source)?;
    get_skill_market_hub_token_status(hub_id)
}

pub fn upsert_skill_market_hub(
    request: SkillMarketHubUpsertRequest,
    source: &str,
) -> Result<SkillMarketHubConfigFile> {
    let now = chrono::Utc::now().to_rfc3339();
    let id = match request.id.as_deref() {
        Some(id) if !id.trim().is_empty() => normalize_hub_id(id)?,
        _ => normalize_hub_id(&slug_from_name(&request.name))?,
    };
    let mut config = read_skill_market_hub_config()?;
    let base_url = normalize_skill_market_endpoint_url(&request.base_url)?;
    let kind = normalize_skill_market_kind(&request.kind);
    let source_type = normalize_skill_market_source_type(&request.source_type);
    let read_only = request.read_only.unwrap_or(false);
    let enabled = request.enabled.unwrap_or(true);
    if let Some(existing) = config.hubs.iter_mut().find(|hub| hub.id == id) {
        if existing.read_only {
            return Err(anyhow!(
                "Read-only skill market hub cannot be modified: {id}"
            ));
        }
        existing.name = request.name.trim().to_string();
        existing.base_url = base_url;
        existing.kind = kind;
        existing.source_type = source_type;
        existing.read_only = read_only;
        existing.enabled = enabled;
        existing.token_ref = Some(skill_market_hub_token_ref(&id));
        existing.updated_at = now;
    } else {
        config.hubs.push(SkillMarketHubConfig {
            id: id.clone(),
            name: request.name.trim().to_string(),
            base_url,
            kind,
            source_type,
            token_ref: if read_only {
                None
            } else {
                Some(skill_market_hub_token_ref(&id))
            },
            read_only,
            enabled,
            created_at: now.clone(),
            updated_at: now,
        });
    }
    write_skill_market_hub_config(&config, source)?;
    read_skill_market_hub_config()
}

pub fn delete_skill_market_hub(hub_id: String, source: &str) -> Result<SkillMarketHubConfigFile> {
    let hub_id = normalize_hub_id(&hub_id)?;
    let mut config = read_skill_market_hub_config()?;
    let hub = config
        .hubs
        .iter()
        .find(|hub| hub.id == hub_id)
        .ok_or_else(|| anyhow!("Skill market hub not found: {hub_id}"))?;
    if hub.read_only {
        return Err(anyhow!(
            "Read-only skill market hub cannot be deleted: {hub_id}"
        ));
    }
    if let Some(token_ref) = &hub.token_ref {
        remove_skill_market_hub_token_by_ref(token_ref)?;
    }
    config.hubs.retain(|hub| hub.id != hub_id);
    config
        .registries
        .retain(|registry| registry.hub_id != hub_id);
    if config.default_hub_id == hub_id {
        config.default_hub_id = DEFAULT_SKILL_MARKET_SOURCES[0].id.to_string();
    }
    write_skill_market_hub_config(&config, source)?;
    read_skill_market_hub_config()
}

pub fn set_skill_market_hub_enabled(
    hub_id: String,
    enabled: bool,
    source: &str,
) -> Result<SkillMarketHubConfigFile> {
    let hub_id = normalize_hub_id(&hub_id)?;
    let mut config = read_skill_market_hub_config()?;
    let hub = config
        .hubs
        .iter_mut()
        .find(|hub| hub.id == hub_id)
        .ok_or_else(|| anyhow!("Skill market hub not found: {hub_id}"))?;
    hub.enabled = enabled;
    hub.updated_at = chrono::Utc::now().to_rfc3339();
    write_skill_market_hub_config(&config, source)?;
    read_skill_market_hub_config()
}

pub fn upsert_skill_market_registry(
    request: SkillMarketRegistryUpsertRequest,
    source: &str,
) -> Result<SkillMarketHubConfigFile> {
    let now = chrono::Utc::now().to_rfc3339();
    let hub_id = normalize_hub_id(&request.hub_id)?;
    let id = match request.id.as_deref() {
        Some(id) if !id.trim().is_empty() => normalize_hub_id(id)?,
        _ => normalize_hub_id(&format!("{}-registry", slug_from_name(&request.name)))?,
    };
    let mut config = read_skill_market_hub_config()?;
    if !config.hubs.iter().any(|hub| hub.id == hub_id) {
        return Err(anyhow!("Skill market hub not found: {hub_id}"));
    }
    let registry_url = normalize_skill_market_endpoint_url(&request.registry_url)?;
    if let Some(existing) = config
        .registries
        .iter_mut()
        .find(|registry| registry.id == id)
    {
        existing.hub_id = hub_id;
        existing.name = request.name.trim().to_string();
        existing.registry_url = registry_url;
        existing.enabled = request.enabled.unwrap_or(existing.enabled);
        existing.updated_at = now;
    } else {
        config.registries.push(SkillMarketRegistryConfig {
            id,
            hub_id,
            name: request.name.trim().to_string(),
            registry_url,
            enabled: request.enabled.unwrap_or(true),
            created_at: now.clone(),
            updated_at: now,
        });
    }
    write_skill_market_hub_config(&config, source)?;
    read_skill_market_hub_config()
}

pub fn delete_skill_market_registry(
    registry_id: String,
    source: &str,
) -> Result<SkillMarketHubConfigFile> {
    let registry_id = normalize_hub_id(&registry_id)?;
    let mut config = read_skill_market_hub_config()?;
    let original_len = config.registries.len();
    config
        .registries
        .retain(|registry| registry.id != registry_id);
    if config.registries.len() == original_len {
        return Err(anyhow!("Skill market registry not found: {registry_id}"));
    }
    write_skill_market_hub_config(&config, source)?;
    read_skill_market_hub_config()
}

pub fn create_skill_publish_draft(request: SkillPublishDraftRequest) -> Result<SkillPublishDraft> {
    let (hub, registry_url) = resolve_publish_hub(&request.hub_id)?;
    let detail = get_skill_detail(&request.skill_name)
        .ok_or_else(|| anyhow!("Skill not found: {}", request.skill_name))?;
    let skill_dir = PathBuf::from(&detail.base_dir)
        .canonicalize()
        .with_context(|| format!("Cannot access skill directory: {}", detail.base_dir))?;
    if !skill_dir.join("SKILL.md").is_file() {
        return Err(anyhow!(
            "Skill '{}' does not have a SKILL.md file.",
            detail.name
        ));
    }
    let manifest_text = fs::read_to_string(skill_dir.join("SKILL.md"))
        .with_context(|| format!("Cannot read SKILL.md for {}", detail.name))?;
    let readme = fs::read_to_string(skill_dir.join("README.md")).unwrap_or_default();
    let hash = skill_file_sha256(&skill_dir).unwrap_or_else(|| "unknown".to_string());
    let token_ref = hub
        .token_ref
        .clone()
        .unwrap_or_else(|| skill_market_hub_token_ref(&hub.id));
    let token_configured = read_skill_market_hub_token_by_ref(&token_ref)?.is_some();
    let publishable = !hub.read_only && hub.enabled && token_configured;
    let error = if hub.read_only {
        Some("Target hub is read-only.".to_string())
    } else if !hub.enabled {
        Some("Target hub is disabled.".to_string())
    } else if !token_configured {
        Some("Target hub token is not configured.".to_string())
    } else {
        None
    };
    Ok(SkillPublishDraft {
        ok: publishable,
        status: if publishable { "ready" } else { "blocked" }.to_string(),
        error,
        skill_name: detail.name.clone(),
        hub_id: hub.id.clone(),
        source_id: hub.id,
        registry_url,
        manifest: serde_json::json!({
            "name": detail.name,
            "version": detail.display.version,
            "description": detail.description,
            "hash": hash,
            "skillMd": manifest_text,
        }),
        readme,
        hash,
        publishable,
        token_required: true,
        token_configured,
    })
}

pub async fn push_skill_to_market_hub(
    request: SkillPublishPushRequest,
) -> Result<SkillPublishPushResult> {
    if !request.confirmed {
        return Err(anyhow!("Skill publish requires explicit confirmation."));
    }
    let draft = create_skill_publish_draft(SkillPublishDraftRequest {
        skill_name: request.skill_name.clone(),
        hub_id: request.hub_id.clone(),
    })?;
    if !draft.publishable {
        return Ok(SkillPublishPushResult {
            ok: false,
            status: "blocked".to_string(),
            error: draft.error,
            skill_name: draft.skill_name,
            source_id: draft.source_id,
            registry_url: draft.registry_url,
            published_at: None,
        });
    }
    let (hub, registry_url) = resolve_publish_hub(&request.hub_id)?;
    let token_ref = hub
        .token_ref
        .clone()
        .unwrap_or_else(|| skill_market_hub_token_ref(&hub.id));
    let token = read_skill_market_hub_token_by_ref(&token_ref)?
        .ok_or_else(|| anyhow!("Target hub token is not configured."))?;
    check_url(&registry_url, SsrfPolicy::Strict, &[]).await?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(Policy::none())
        .user_agent("HopeAgent/skill-market-publisher")
        .build()
        .context("Cannot create skill market publish HTTP client")?;
    let response = client
        .post(&registry_url)
        .bearer_auth(token)
        .json(&serde_json::json!({
            "skillName": draft.skill_name,
            "hubId": draft.hub_id,
            "hash": draft.hash,
            "manifest": draft.manifest,
            "readme": draft.readme,
        }))
        .send()
        .await
        .with_context(|| format!("Cannot publish skill to hub {}", hub.name))?;
    let status = response.status();
    if response.content_length().unwrap_or(0) > SKILL_MARKET_PUBLISH_RESPONSE_MAX_BYTES {
        return Err(anyhow!("Skill market publish response is too large."));
    }
    let body = response
        .bytes()
        .await
        .context("Cannot read skill market publish response")?;
    if body.len() as u64 > SKILL_MARKET_PUBLISH_RESPONSE_MAX_BYTES {
        return Err(anyhow!("Skill market publish response is too large."));
    }
    if !status.is_success() {
        let message = String::from_utf8_lossy(&body);
        return Ok(SkillPublishPushResult {
            ok: false,
            status: "failed".to_string(),
            error: Some(format!(
                "Hub returned HTTP {}: {}",
                status,
                crate::truncate_utf8(message.trim(), 512)
            )),
            skill_name: draft.skill_name,
            source_id: hub.id,
            registry_url,
            published_at: None,
        });
    }
    Ok(SkillPublishPushResult {
        ok: true,
        status: "published".to_string(),
        error: None,
        skill_name: draft.skill_name,
        source_id: hub.id,
        registry_url,
        published_at: Some(chrono::Utc::now().to_rfc3339()),
    })
}

fn skill_market_hubs_path() -> Result<PathBuf> {
    Ok(crate::paths::root_dir()?.join("skill-market-hubs.json"))
}

fn read_skill_market_hub_config() -> Result<SkillMarketHubConfigFile> {
    let path = skill_market_hubs_path()?;
    if !path.is_file() {
        return Ok(default_skill_market_hub_config());
    }
    let text = fs::read_to_string(&path)
        .with_context(|| format!("Cannot read skill market hubs from {}", path.display()))?;
    let parsed: SkillMarketHubConfigFile = serde_json::from_str(&text)
        .with_context(|| format!("Cannot parse skill market hubs from {}", path.display()))?;
    normalize_skill_market_hub_config(parsed)
}

fn write_skill_market_hub_config(config: &SkillMarketHubConfigFile, source: &str) -> Result<()> {
    let normalized = normalize_skill_market_hub_config(config.clone())?;
    let path = skill_market_hubs_path()?;
    let payload =
        serde_json::to_string_pretty(&normalized).context("Cannot serialize skill market hubs")?;
    fs::write(&path, payload)
        .with_context(|| format!("Cannot write skill market hubs from {}", source))?;
    Ok(())
}

fn default_skill_market_hub_config() -> SkillMarketHubConfigFile {
    let now = chrono::Utc::now().to_rfc3339();
    SkillMarketHubConfigFile {
        default_hub_id: DEFAULT_SKILL_MARKET_SOURCES[0].id.to_string(),
        hubs: DEFAULT_SKILL_MARKET_SOURCES
            .iter()
            .map(|source| SkillMarketHubConfig {
                id: source.id.to_string(),
                name: source.name.to_string(),
                base_url: source.url.to_string(),
                kind: "clawhub".to_string(),
                source_type: source.source_type.to_string(),
                token_ref: None,
                read_only: true,
                enabled: true,
                created_at: now.clone(),
                updated_at: now.clone(),
            })
            .collect(),
        registries: Vec::new(),
    }
}

fn normalize_skill_market_hub_config(
    mut config: SkillMarketHubConfigFile,
) -> Result<SkillMarketHubConfigFile> {
    let mut default = default_skill_market_hub_config();
    let mut seen = HashSet::new();
    let now = chrono::Utc::now().to_rfc3339();
    for hub in &default.hubs {
        seen.insert(hub.id.clone());
    }
    for hub in &mut config.hubs {
        hub.id = normalize_hub_id(&hub.id)?;
        hub.name = hub.name.trim().to_string();
        if hub.name.is_empty() {
            hub.name = hub.id.clone();
        }
        hub.base_url = normalize_skill_market_endpoint_url(&hub.base_url)?;
        hub.kind = normalize_skill_market_kind(&hub.kind);
        hub.source_type = normalize_skill_market_source_type(&hub.source_type);
        if !hub.read_only && hub.token_ref.is_none() {
            hub.token_ref = Some(skill_market_hub_token_ref(&hub.id));
        }
        if hub.created_at.trim().is_empty() {
            hub.created_at = now.clone();
        }
        if hub.updated_at.trim().is_empty() {
            hub.updated_at = now.clone();
        }
    }
    default.hubs.extend(
        config
            .hubs
            .into_iter()
            .filter(|hub| seen.insert(hub.id.clone())),
    );
    for registry in &mut config.registries {
        registry.id = normalize_hub_id(&registry.id)?;
        registry.hub_id = normalize_hub_id(&registry.hub_id)?;
        registry.name = registry.name.trim().to_string();
        if registry.name.is_empty() {
            registry.name = registry.id.clone();
        }
        registry.registry_url = normalize_skill_market_endpoint_url(&registry.registry_url)?;
        if registry.created_at.trim().is_empty() {
            registry.created_at = now.clone();
        }
        if registry.updated_at.trim().is_empty() {
            registry.updated_at = now.clone();
        }
    }
    let known_hubs: HashSet<String> = default.hubs.iter().map(|hub| hub.id.clone()).collect();
    default.registries = config
        .registries
        .into_iter()
        .filter(|registry| known_hubs.contains(&registry.hub_id))
        .collect();
    default.default_hub_id = if known_hubs.contains(&config.default_hub_id) {
        config.default_hub_id
    } else {
        DEFAULT_SKILL_MARKET_SOURCES[0].id.to_string()
    };
    Ok(default)
}

fn normalize_hub_id(id: &str) -> Result<String> {
    let trimmed = id.trim();
    if trimmed.is_empty()
        || trimmed.len() > 64
        || !trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(anyhow!("Invalid skill market hub id: {trimmed}"));
    }
    Ok(trimmed.to_string())
}

fn normalize_skill_market_endpoint_url(url: &str) -> Result<String> {
    let trimmed = url.trim();
    let parsed = reqwest::Url::parse(trimmed)
        .with_context(|| format!("Invalid skill market endpoint URL: {trimmed}"))?;
    match parsed.scheme() {
        "https" | "http" => Ok(trimmed.to_string()),
        _ => Err(anyhow!("Skill market endpoint must be http(s): {trimmed}")),
    }
}

fn normalize_skill_market_kind(kind: &str) -> String {
    match kind.trim() {
        "clawhub" | "skillhub" | "generic" => kind.trim().to_string(),
        _ => "generic".to_string(),
    }
}

fn normalize_skill_market_source_type(source_type: &str) -> String {
    match source_type.trim() {
        "clawhub-lock" | "skillhub" | "registry" => source_type.trim().to_string(),
        _ => "clawhub-lock".to_string(),
    }
}

fn resolve_publish_hub(hub_id: &str) -> Result<(SkillMarketHubConfig, String)> {
    let hub_id = normalize_hub_id(hub_id)?;
    let config = read_skill_market_hub_config()?;
    let hub = config
        .hubs
        .iter()
        .find(|hub| hub.id == hub_id)
        .cloned()
        .ok_or_else(|| anyhow!("Skill market hub not found: {hub_id}"))?;
    let registry_url = config
        .registries
        .iter()
        .find(|registry| registry.hub_id == hub.id && registry.enabled)
        .map(|registry| registry.registry_url.clone())
        .unwrap_or_else(|| hub.base_url.clone());
    Ok((hub, registry_url))
}

fn slug_from_name(name: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in name.trim().chars() {
        let next = if ch.is_ascii_alphanumeric() {
            last_dash = false;
            Some(ch.to_ascii_lowercase())
        } else if !last_dash {
            last_dash = true;
            Some('-')
        } else {
            None
        };
        if let Some(ch) = next {
            slug.push(ch);
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "skillhub".to_string()
    } else {
        slug
    }
}

fn skill_market_hub_token_ref(hub_id: &str) -> String {
    format!("skill-market-hub:{hub_id}")
}

fn skill_market_hub_token_path(token_ref: &str) -> Result<PathBuf> {
    let suffix = token_ref
        .strip_prefix("skill-market-hub:")
        .ok_or_else(|| anyhow!("Invalid skill market token ref."))?;
    let hub_id = normalize_hub_id(suffix)?;
    Ok(crate::paths::credentials_dir()?
        .join("skill-market")
        .join(format!("{hub_id}.json")))
}

fn read_skill_market_hub_token_by_ref(token_ref: &str) -> Result<Option<String>> {
    let path = skill_market_hub_token_path(token_ref)?;
    if !path.is_file() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let credential: SkillMarketHubTokenCredential =
        serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    let token = credential.token.trim();
    if token.is_empty() {
        Ok(None)
    } else {
        Ok(Some(token.to_string()))
    }
}

fn write_skill_market_hub_token_by_ref(token_ref: &str, token: &str) -> Result<()> {
    let path = skill_market_hub_token_path(token_ref)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let payload = serde_json::to_vec_pretty(&SkillMarketHubTokenCredential {
        token: token.to_string(),
        updated_at: chrono::Utc::now().to_rfc3339(),
    })?;
    crate::platform::write_secure_file(&path, &payload)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn remove_skill_market_hub_token_by_ref(token_ref: &str) -> Result<()> {
    let path = skill_market_hub_token_path(token_ref)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("remove {}", path.display())),
    }
}

fn mask_skill_market_token(token: &str) -> String {
    mask_value(token)
}

fn skill_market_sources_path() -> Result<PathBuf> {
    Ok(crate::paths::root_dir()?.join("skill-market-sources.json"))
}

fn read_skill_market_sources() -> Result<Vec<String>> {
    let path = skill_market_sources_path()?;
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path)
        .with_context(|| format!("Cannot read skill market sources from {}", path.display()))?;
    let sources: Vec<String> = serde_json::from_str(&text)
        .with_context(|| format!("Cannot parse skill market sources from {}", path.display()))?;
    normalize_skill_market_sources(sources)
}

fn write_skill_market_sources(source_urls: &[String], source: &str) -> Result<()> {
    let path = skill_market_sources_path()?;
    let payload = serde_json::to_string_pretty(source_urls)
        .context("Cannot serialize skill market sources")?;
    fs::write(&path, payload)
        .with_context(|| format!("Cannot write skill market sources from {}", source))?;
    Ok(())
}

fn normalize_skill_market_sources(source_urls: Vec<String>) -> Result<Vec<String>> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for url in source_urls {
        let trimmed = url.trim();
        if trimmed.is_empty() || trimmed == CLAWHUB_SKILLS_LOCK_URL {
            continue;
        }
        let parsed = reqwest::Url::parse(trimmed)
            .with_context(|| format!("Invalid skill market source URL: {trimmed}"))?;
        match parsed.scheme() {
            "https" | "http" => {}
            _ => return Err(anyhow!("Skill market source must be http(s): {trimmed}")),
        }
        if seen.insert(trimmed.to_string()) {
            normalized.push(trimmed.to_string());
        }
        if normalized.len() > REMOTE_MARKET_MAX_SOURCES {
            return Err(anyhow!(
                "At most {} custom skill market sources are supported.",
                REMOTE_MARKET_MAX_SOURCES
            ));
        }
    }
    Ok(normalized)
}

async fn fetch_clawhub_market_entries(
    client: &reqwest::Client,
    source_id: &str,
    source_name: &str,
    source_url: &str,
    installed_by_name: &HashMap<String, SkillSummary>,
) -> Result<Vec<SkillRemoteMarketEntry>> {
    validate_clawhub_market_index_url(source_url).await?;
    let response = client
        .get(source_url)
        .send()
        .await
        .with_context(|| format!("Cannot fetch {source_name} skill market index"))?;
    if !response.status().is_success() {
        return Err(anyhow!(
            "{} skill market returned HTTP {}.",
            source_name,
            response.status()
        ));
    }
    if response.content_length().unwrap_or(0) > REMOTE_MARKET_MAX_BYTES {
        return Err(anyhow!("{} skill market index is too large.", source_name));
    }
    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("Cannot read {} skill market index", source_name))?;
    if bytes.len() as u64 > REMOTE_MARKET_MAX_BYTES {
        return Err(anyhow!("{} skill market index is too large.", source_name));
    }
    let lock: ClawHubSkillsLock = serde_json::from_slice(&bytes)
        .with_context(|| format!("Cannot parse {} skill market index", source_name))?;

    let mut entries = Vec::new();
    for (id, entry) in lock.skills.into_iter().take(REMOTE_MARKET_MAX_ENTRIES) {
        if entry.source_type != "github" {
            continue;
        }
        let normalized_skill_path = entry.skill_path.replace('\\', "/");
        let raw_url = format!(
            "https://github.com/{}/blob/HEAD/{}",
            entry.source,
            normalized_skill_path
        );
        let metadata = fetch_remote_skill_metadata(client, &entry.source, &normalized_skill_path)
            .await
            .unwrap_or_default();
        let entry_name = metadata.name.clone().unwrap_or_else(|| id.clone());
        let installed_skill = installed_by_name
            .get(&entry_name)
            .or_else(|| installed_by_name.get(&id));
        let installed = installed_skill.is_some();
        let installed_version = installed_skill.and_then(|skill| skill.display.version.clone());
        let installed_hash =
            installed_skill.and_then(|skill| skill_file_sha256(Path::new(&skill.base_dir)));
        let market_hash = entry.computed_hash.clone();
        let market_version = metadata.version.clone();
        let (update_available, comparison_basis, update_reason) = compare_market_skill(
            installed,
            installed_version.as_deref(),
            market_version.as_deref(),
            installed_hash.as_deref(),
            market_hash.as_deref(),
        );
        let installed_state = if update_available {
            "updateAvailable"
        } else if installed {
            "installed"
        } else {
            "available"
        }
        .to_string();
        let category = metadata
            .category
            .clone()
            .unwrap_or_else(|| skill_market_category(&entry_name));
        let tags = skill_market_tags_with_metadata(&entry_name, &category, &metadata.tags);
        let actions = if installed {
            let mut actions = vec!["inspect".to_string()];
            if update_available {
                actions.push("update".to_string());
            }
            actions
        } else {
            vec!["inspect".to_string(), "install".to_string()]
        };
        entries.push(SkillRemoteMarketEntry {
            id: format!("{}:{id}", source_id),
            source_id: source_id.to_string(),
            source_name: source_name.to_string(),
            name: entry_name.clone(),
            source: entry.source.clone(),
            source_type: entry.source_type,
            skill_path: entry.skill_path.clone(),
            raw_url,
            description: metadata.description.unwrap_or_else(|| {
                skill_market_description(&entry_name, &entry.source, &entry.skill_path)
            }),
            author: metadata.author.unwrap_or_else(|| skill_market_author(&entry.source)),
            license: metadata.license.unwrap_or_else(|| "unknown".to_string()),
            category,
            tags,
            rating: skill_market_rating(&id),
            download_count: skill_market_download_count(&id),
            updated_at: metadata.updated_at,
            featured: skill_market_featured(&id),
            compatible_apps: external_skill_app_kinds(),
            market_version,
            installed_version,
            market_hash,
            installed_hash,
            comparison_basis,
            installed,
            update_available,
            update_reason,
            installed_state,
            actions,
        });
    }
    Ok(entries)
}

fn read_skill_display_version(skill_dir: &Path) -> Option<String> {
    let content = fs::read_to_string(skill_dir.join("SKILL.md")).ok()?;
    let value = content
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            trimmed
                .strip_prefix("version:")
                .or_else(|| trimmed.strip_prefix("display.version:"))
        })?
        .trim()
        .trim_matches(['\'', '"']);
    (!value.is_empty()).then(|| value.to_string())
}

fn skill_dir_updated_at(skill_dir: &Path) -> Option<String> {
    let modified = fs::metadata(skill_dir.join("SKILL.md"))
        .or_else(|_| fs::metadata(skill_dir))
        .ok()?
        .modified()
        .ok()?;
    let datetime: chrono::DateTime<chrono::Utc> = modified.into();
    Some(datetime.to_rfc3339())
}

fn skill_market_description(name: &str, source: &str, skill_path: &str) -> String {
    format!(
        "{} from {} ({})",
        name.replace(['-', '_'], " "),
        source,
        skill_path.replace('\\', "/")
    )
}

fn skill_market_author(source: &str) -> String {
    source
        .split('/')
        .next()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("community")
        .to_string()
}

fn skill_market_category(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    if lower.contains("prompt")
        || lower.contains("eval")
        || lower.contains("lab")
        || lower.contains("test")
    {
        "测试".to_string()
    } else if lower.contains("doc")
        || lower.contains("readme")
        || lower.contains("write")
        || lower.contains("spec")
    {
        "文档".to_string()
    } else if lower.contains("deploy")
        || lower.contains("release")
        || lower.contains("ci")
        || lower.contains("docker")
    {
        "自动化".to_string()
    } else if lower.contains("ui")
        || lower.contains("design")
        || lower.contains("inspect")
        || lower.contains("ux")
    {
        "设计".to_string()
    } else if lower.contains("data")
        || lower.contains("sql")
        || lower.contains("analytics")
        || lower.contains("report")
    {
        "数据".to_string()
    } else if lower.contains("team")
        || lower.contains("meeting")
        || lower.contains("status")
        || lower.contains("collab")
    {
        "团队协作".to_string()
    } else if lower.contains("workflow")
        || lower.contains("agent")
        || lower.contains("task")
        || lower.contains("cron")
    {
        "效率".to_string()
    } else {
        "开发".to_string()
    }
}

fn skill_market_tags(name: &str) -> Vec<String> {
    let category = skill_market_category(name);
    let mut tags = vec![category];
    if name.contains('-') {
        tags.extend(
            name.split('-')
                .filter(|part| !part.trim().is_empty())
                .take(2)
                .map(|part| part.to_string()),
        );
    }
    tags.sort();
    tags.dedup();
    tags
}

fn skill_market_tags_with_metadata(
    name: &str,
    category: &str,
    metadata_tags: &[String],
) -> Vec<String> {
    let mut tags = vec![category.to_string()];
    tags.extend(metadata_tags.iter().cloned());
    if name.contains('-') {
        tags.extend(
            name.split('-')
                .filter(|part| !part.trim().is_empty())
                .take(2)
                .map(|part| part.to_string()),
        );
    }
    tags.sort();
    tags.dedup();
    tags
}

async fn validate_clawhub_market_index_url(url: &str) -> Result<()> {
    let parsed = url::Url::parse(url).with_context(|| format!("Invalid market index URL: {url}"))?;
    let is_default_clawhub = parsed.scheme() == "https"
        && parsed.host_str() == Some(GITHUB_RAW_HOST)
        && parsed.path() == "/openclaw/clawhub/main/skills-lock.json"
        && parsed.query().is_none()
        && parsed.fragment().is_none();
    if is_default_clawhub {
        check_url(url, SsrfPolicy::Strict, &github_market_allowed_hosts()).await?;
        return Ok(());
    }
    check_url(url, SsrfPolicy::Strict, &[]).await?;
    Ok(())
}

async fn validate_remote_skill_metadata_url(
    raw_url: &str,
    owner: &str,
    repo: &str,
    skill_path: &str,
) -> Result<()> {
    let parsed =
        url::Url::parse(raw_url).with_context(|| format!("Invalid skill metadata URL: {raw_url}"))?;
    let expected_path = format!("/{owner}/{repo}/HEAD/{skill_path}");
    let is_allowed_github_raw = parsed.scheme() == "https"
        && parsed.host_str() == Some(GITHUB_RAW_HOST)
        && parsed.path() == expected_path
        && parsed.query().is_none()
        && parsed.fragment().is_none()
        && is_safe_github_slug(owner)
        && is_safe_github_slug(repo)
        && is_safe_market_skill_path(skill_path);
    if is_allowed_github_raw {
        check_url(raw_url, SsrfPolicy::Strict, &github_market_allowed_hosts()).await?;
        return Ok(());
    }
    check_url(raw_url, SsrfPolicy::Strict, &[]).await?;
    Ok(())
}

fn github_market_allowed_hosts() -> Vec<String> {
    GITHUB_MARKET_ALLOWED_HOSTS
        .iter()
        .map(|host| (*host).to_string())
        .collect()
}

fn is_safe_github_slug(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 100
        && trimmed
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn is_safe_market_skill_path(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && !trimmed.starts_with('/')
        && !trimmed.contains('\\')
        && trimmed
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

async fn fetch_remote_skill_metadata(
    client: &reqwest::Client,
    source: &str,
    skill_path: &str,
) -> Result<SkillRemoteMarketMetadata> {
    let Some((owner, repo)) = source.split_once('/') else {
        return Ok(SkillRemoteMarketMetadata::default());
    };
    if owner.trim().is_empty() || repo.trim().is_empty() {
        return Ok(SkillRemoteMarketMetadata::default());
    }
    let raw_url = format!(
        "https://raw.githubusercontent.com/{}/{}/HEAD/{}",
        owner.trim(),
        repo.trim(),
        skill_path.trim_start_matches('/')
    );
    validate_remote_skill_metadata_url(
        &raw_url,
        owner.trim(),
        repo.trim(),
        skill_path.trim_start_matches('/'),
    )
    .await?;
    let response = client
        .get(&raw_url)
        .send()
        .await
        .with_context(|| format!("Cannot fetch remote skill metadata from {raw_url}"))?;
    if !response.status().is_success() {
        return Err(anyhow!(
            "Remote skill metadata returned HTTP {}.",
            response.status()
        ));
    }
    if response.content_length().unwrap_or(0) > REMOTE_MARKET_MAX_BYTES {
        return Err(anyhow!("Remote skill metadata is too large."));
    }
    let bytes = response
        .bytes()
        .await
        .context("Cannot read remote skill metadata")?;
    if bytes.len() as u64 > REMOTE_MARKET_MAX_BYTES {
        return Err(anyhow!("Remote skill metadata is too large."));
    }
    let content = String::from_utf8_lossy(&bytes);
    Ok(parse_remote_skill_metadata(&content))
}

fn parse_remote_skill_metadata(content: &str) -> SkillRemoteMarketMetadata {
    let mut metadata = SkillRemoteMarketMetadata::default();
    let Some(frontmatter) = markdown_frontmatter(content) else {
        metadata.description = first_markdown_paragraph(content);
        return metadata;
    };
    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('-') {
            continue;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = clean_frontmatter_value(value);
        if value.is_empty() {
            continue;
        }
        match key.as_str() {
            "name" => metadata.name = Some(value),
            "description" => metadata.description = Some(value),
            "author" | "authors" | "authored_by" | "authoredby" => metadata.author = Some(value),
            "license" => metadata.license = Some(value),
            "version" => metadata.version = Some(value),
            "category" => metadata.category = Some(value),
            "tags" | "categories" => metadata.tags.extend(parse_frontmatter_list(&value)),
            "updated_at" | "updatedat" | "updated" | "modified" => metadata.updated_at = Some(value),
            _ => {}
        }
    }
    if metadata.description.is_none() {
        metadata.description = first_markdown_paragraph(content);
    }
    metadata.tags.sort();
    metadata.tags.dedup();
    metadata
}

fn markdown_frontmatter(content: &str) -> Option<&str> {
    let rest = content.strip_prefix("---")?;
    let rest = rest.strip_prefix('\n').or_else(|| rest.strip_prefix("\r\n"))?;
    rest.split_once("\n---")
        .map(|(frontmatter, _)| frontmatter)
        .or_else(|| rest.split_once("\r\n---").map(|(frontmatter, _)| frontmatter))
}

fn clean_frontmatter_value(value: &str) -> String {
    value
        .trim()
        .trim_matches(['\'', '"'])
        .trim()
        .to_string()
}

fn parse_frontmatter_list(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(trimmed);
    inner
        .split(',')
        .map(clean_frontmatter_value)
        .filter(|value| !value.is_empty())
        .collect()
}

fn first_markdown_paragraph(content: &str) -> Option<String> {
    content
        .lines()
        .map(str::trim)
        .find(|line| {
            !line.is_empty()
                && !line.starts_with('#')
                && !line.starts_with("---")
                && !line.starts_with('`')
                && !line.starts_with('|')
        })
        .map(|line| crate::truncate_utf8(line, 240).to_string())
}

fn skill_market_rating(name: &str) -> f32 {
    let _ = name;
    0.0
}

fn skill_market_download_count(name: &str) -> u64 {
    let _ = name;
    0
}

fn skill_market_featured(name: &str) -> bool {
    let _ = name;
    false
}

fn external_skill_app_kinds() -> Vec<String> {
    vec![
        "claude".to_string(),
        "codex".to_string(),
        "gemini".to_string(),
        "opencode".to_string(),
    ]
}

fn update_skill_market_source_stats(
    source: &mut SkillRemoteMarketSource,
    entries: &[SkillRemoteMarketEntry],
) {
    source.entry_count = entries.len();
    source.category_counts = skill_market_category_counts(entries);
    source.installed_count = entries.iter().filter(|entry| entry.installed).count();
    source.update_count = entries
        .iter()
        .filter(|entry| entry.update_available)
        .count();
}

fn skill_market_category_counts(entries: &[SkillRemoteMarketEntry]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for entry in entries {
        *counts.entry(entry.category.clone()).or_insert(0) += 1;
    }
    counts
}

pub fn install_registry_skill(
    skill_path: String,
    name: Option<String>,
) -> Result<SkillRegistryInstallReport> {
    let source = PathBuf::from(&skill_path)
        .canonicalize()
        .with_context(|| format!("Cannot access registry skill directory: {skill_path}"))?;
    if !source.is_dir() || !source.join("SKILL.md").is_file() {
        return Err(anyhow!(
            "Registry skill source must be a directory containing SKILL.md."
        ));
    }

    let skill_name = match name {
        Some(n) if !n.trim().is_empty() => n.trim().to_string(),
        _ => source
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| anyhow!("Cannot infer skill name from source path."))?
            .to_string(),
    };
    validate_skill_dir_name(&skill_name)?;
    let target_root = crate::paths::skills_dir()?;
    fs::create_dir_all(&target_root)
        .with_context(|| format!("Cannot create skills directory: {}", target_root.display()))?;
    let target = target_root.join(&skill_name);
    if target.exists() {
        return Err(anyhow!(
            "Skill '{}' already exists at {}; refusing to overwrite.",
            skill_name,
            target.display()
        ));
    }
    copy_skill_dir(&source, &target)?;
    bump_skill_version();
    Ok(SkillRegistryInstallReport {
        name: skill_name,
        source_path: source.to_string_lossy().to_string(),
        target_path: target.to_string_lossy().to_string(),
        installed: true,
        updated: false,
        backup_path: None,
    })
}

pub fn update_registry_skill(
    skill_path: String,
    name: Option<String>,
) -> Result<SkillRegistryInstallReport> {
    let source = PathBuf::from(&skill_path)
        .canonicalize()
        .with_context(|| format!("Cannot access registry skill directory: {skill_path}"))?;
    if !source.is_dir() || !source.join("SKILL.md").is_file() {
        return Err(anyhow!(
            "Registry skill source must be a directory containing SKILL.md."
        ));
    }

    let skill_name = match name {
        Some(n) if !n.trim().is_empty() => n.trim().to_string(),
        _ => source
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| anyhow!("Cannot infer skill name from source path."))?
            .to_string(),
    };
    validate_skill_dir_name(&skill_name)?;

    let target_root = crate::paths::skills_dir()?;
    let root = target_root
        .canonicalize()
        .with_context(|| "Cannot access managed skills directory.".to_string())?;
    let target = root.join(&skill_name);
    let canonical_target = target.canonicalize().with_context(|| {
        format!(
            "Managed skill '{}' does not exist; install it before updating.",
            skill_name
        )
    })?;
    if !canonical_target.starts_with(&root) || !canonical_target.join("SKILL.md").is_file() {
        return Err(anyhow!(
            "Refusing to update '{}': target is not a managed skill directory.",
            skill_name
        ));
    }
    if source == canonical_target {
        return Err(anyhow!(
            "Registry source and managed target are the same directory; nothing to update."
        ));
    }
    if !skill_content_differs(&source, &canonical_target) {
        return Ok(SkillRegistryInstallReport {
            name: skill_name,
            source_path: source.to_string_lossy().to_string(),
            target_path: canonical_target.to_string_lossy().to_string(),
            installed: true,
            updated: false,
            backup_path: None,
        });
    }

    let backup = root.join(format!(
        ".{}.backup.{}",
        skill_name,
        chrono::Utc::now().format("%Y%m%d%H%M%S")
    ));
    replace_skill_dir(&source, &canonical_target, &backup)?;
    bump_skill_version();
    Ok(SkillRegistryInstallReport {
        name: skill_name,
        source_path: source.to_string_lossy().to_string(),
        target_path: canonical_target.to_string_lossy().to_string(),
        installed: true,
        updated: true,
        backup_path: Some(backup.to_string_lossy().to_string()),
    })
}

pub fn install_skill_to_app(name: String, app: String) -> Result<SkillAppInstallReport> {
    let detail = get_skill_detail(&name).ok_or_else(|| anyhow!("Skill not found: {name}"))?;
    let source = PathBuf::from(&detail.base_dir)
        .canonicalize()
        .with_context(|| format!("Cannot access skill directory: {}", detail.base_dir))?;
    if !source.join("SKILL.md").is_file() {
        return Err(anyhow!(
            "Skill '{}' does not have a SKILL.md file.",
            detail.name
        ));
    }
    let target_root = external_app_skills_root(&app)
        .ok_or_else(|| anyhow!("Unsupported external skills app: {app}"))?;
    fs::create_dir_all(&target_root).with_context(|| {
        format!(
            "Cannot create app skills directory: {}",
            target_root.display()
        )
    })?;
    let target = target_root.join(&detail.name);
    if target.exists() {
        return Err(anyhow!(
            "Target app skill already exists at {}; refusing to overwrite.",
            target.display()
        ));
    }
    copy_skill_dir(&source, &target)?;
    Ok(SkillAppInstallReport {
        skill_name: detail.name,
        app,
        source_path: source.to_string_lossy().to_string(),
        target_path: target.to_string_lossy().to_string(),
        installed: true,
    })
}

pub fn uninstall_managed_skill(name: String) -> Result<SkillUninstallReport> {
    validate_skill_dir_name(&name)?;
    let root = crate::paths::skills_dir()?
        .canonicalize()
        .with_context(|| "Cannot access managed skills directory.".to_string())?;
    let target = root.join(&name);
    let canonical = target
        .canonicalize()
        .with_context(|| format!("Managed skill '{}' does not exist.", name))?;
    if !canonical.starts_with(&root) || !canonical.join("SKILL.md").is_file() {
        return Err(anyhow!(
            "Refusing to uninstall '{}': target is not a managed skill directory.",
            name
        ));
    }
    fs::remove_dir_all(&canonical)
        .with_context(|| format!("Cannot remove managed skill: {}", canonical.display()))?;
    bump_skill_version();
    Ok(SkillUninstallReport {
        skill_name: name,
        removed_path: canonical.to_string_lossy().to_string(),
        removed: true,
    })
}

pub fn uninstall_skill_from_app(name: String, app: String) -> Result<SkillUninstallReport> {
    validate_skill_dir_name(&name)?;
    if app == "hope" {
        return Err(anyhow!(
            "Use uninstall_managed_skill for Hope managed skills; external app uninstall does not accept app=hope."
        ));
    }
    let root = external_app_skills_root(&app)
        .ok_or_else(|| anyhow!("Unsupported external skills app: {app}"))?
        .canonicalize()
        .with_context(|| format!("Cannot access external app skills directory for {app}."))?;
    let target = root.join(&name);
    let canonical = target.canonicalize().with_context(|| {
        format!(
            "External app skill '{}' is not installed for {}.",
            name, app
        )
    })?;
    if !canonical.starts_with(&root) || canonical == root || !canonical.join("SKILL.md").is_file() {
        return Err(anyhow!(
            "Refusing to uninstall '{}' from '{}': target is not a skill directory under the app skills root.",
            name,
            app
        ));
    }
    fs::remove_dir_all(&canonical)
        .with_context(|| format!("Cannot remove external app skill: {}", canonical.display()))?;
    Ok(SkillUninstallReport {
        skill_name: name,
        removed_path: canonical.to_string_lossy().to_string(),
        removed: true,
    })
}

pub fn scan_skill_usage(db: &crate::session::SessionDB) -> Result<SkillUsageScanReport> {
    let apps = probe_skill_apps();
    let skills_by_name: HashMap<String, SkillSummary> = list_skills()
        .into_iter()
        .map(|skill| (skill.name.clone(), skill))
        .collect();
    let mut usage_by_name: HashMap<String, SkillUsageSnapshot> = HashMap::new();
    let mut trend_by_day_app: HashMap<(String, String), u64> = HashMap::new();
    let mut recent_usage = Vec::new();
    let mut app_counts: HashMap<String, u64> = HashMap::new();

    for (skill_name, usage_count, last_used_at) in db.aggregate_skill_activations()? {
        let count = usage_count.max(0) as u64;
        let apps_for_skill = skills_by_name
            .get(&skill_name)
            .map(|skill| app_install_states_for_skill(skill, &apps))
            .unwrap_or_default();
        usage_by_name.insert(
            skill_name.clone(),
            SkillUsageSnapshot {
                skill_name,
                usage_count: count,
                last_used_at,
                apps: apps_for_skill,
            },
        );
        *app_counts.entry("hope".to_string()).or_default() += count;
    }

    for (skill_name, session_id, activated_at) in db.recent_skill_activations(200)? {
        let date = usage_date_key(&activated_at);
        *trend_by_day_app
            .entry((date, "hope".to_string()))
            .or_default() += 1;
        recent_usage.push(SkillUsageRecentRecord {
            activated_at,
            app: "hope".to_string(),
            skill_name,
            session_id,
            count: 1,
        });
    }

    let external_usage = scan_external_skill_usage(&skills_by_name, &apps);
    for sample in external_usage.records {
        let entry = usage_by_name
            .entry(sample.skill_name.clone())
            .or_insert_with(|| SkillUsageSnapshot {
                skill_name: sample.skill_name.clone(),
                usage_count: 0,
                last_used_at: None,
                apps: skills_by_name
                    .get(&sample.skill_name)
                    .map(|skill| app_install_states_for_skill(skill, &apps))
                    .unwrap_or_default(),
            });
        entry.usage_count = entry.usage_count.saturating_add(sample.count);
        entry.last_used_at = newer_timestamp(entry.last_used_at.take(), Some(sample.activated_at.clone()));
        *app_counts.entry(sample.app.clone()).or_default() += sample.count;
        *trend_by_day_app
            .entry((usage_date_key(&sample.activated_at), sample.app.clone()))
            .or_default() += sample.count;
        recent_usage.push(sample);
    }

    let mut usage: Vec<_> = usage_by_name.into_values().collect();
    usage.sort_by(|left, right| {
        right
            .usage_count
            .cmp(&left.usage_count)
            .then_with(|| left.skill_name.cmp(&right.skill_name))
    });
    let mut usage_trend: Vec<_> = trend_by_day_app
        .into_iter()
        .map(|((date, app), count)| SkillUsageTrendPoint { date, app, count })
        .collect();
    usage_trend.sort_by(|left, right| left.date.cmp(&right.date).then_with(|| left.app.cmp(&right.app)));
    recent_usage.sort_by(|left, right| right.activated_at.cmp(&left.activated_at));
    recent_usage.truncate(200);
    let mut usage_app_breakdown: Vec<_> = app_counts
        .into_iter()
        .map(|(app, count)| SkillUsageAppBreakdown { app, count })
        .collect();
    usage_app_breakdown.sort_by(|left, right| right.count.cmp(&left.count).then_with(|| left.app.cmp(&right.app)));

    Ok(SkillUsageScanReport {
        usage,
        usage_trend,
        recent_usage,
        usage_app_breakdown,
        scanned_at: chrono::Utc::now().to_rfc3339(),
        source: "session_skill_activation+external_app_logs".to_string(),
    })
}


#[derive(Debug, Clone)]
struct ExternalSkillUsageSample {
    app: String,
    skill_name: String,
    activated_at: String,
    session_id: String,
    count: u64,
}

#[derive(Debug, Default)]
struct ExternalSkillUsageScan {
    records: Vec<SkillUsageRecentRecord>,
}

fn scan_external_skill_usage(
    skills_by_name: &HashMap<String, SkillSummary>,
    apps: &[SkillAppProbe],
) -> ExternalSkillUsageScan {
    let skill_names: Vec<String> = skills_by_name.keys().cloned().collect();
    if skill_names.is_empty() {
        return ExternalSkillUsageScan::default();
    }
    let mut samples = Vec::new();
    for app in ["claude", "codex", "gemini", "opencode"] {
        if !apps.iter().any(|probe| probe.app == app && probe.installed) {
            continue;
        }
        for root in external_chat_roots(app) {
            collect_external_skill_usage_from_root(app, &root, &skill_names, &mut samples);
        }
    }
    let mut by_key: HashMap<(String, String, String, String), u64> = HashMap::new();
    for sample in samples {
        *by_key
            .entry((
                sample.app,
                sample.skill_name,
                usage_date_key(&sample.activated_at),
                sample.session_id,
            ))
            .or_default() += sample.count;
    }
    let mut records = by_key
        .into_iter()
        .map(|((app, skill_name, date, session_id), count)| SkillUsageRecentRecord {
            activated_at: format!("{date}T00:00:00Z"),
            app,
            skill_name,
            session_id,
            count,
        })
        .collect::<Vec<_>>();
    records.sort_by(|left, right| right.activated_at.cmp(&left.activated_at));
    records.truncate(500);
    ExternalSkillUsageScan { records }
}

fn collect_external_skill_usage_from_root(
    app: &str,
    root: &Path,
    skill_names: &[String],
    out: &mut Vec<ExternalSkillUsageSample>,
) {
    if !root.is_dir() {
        return;
    }
    let mut stack = vec![root.to_path_buf()];
    let mut scanned_files = 0usize;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = entry.metadata() else { continue };
            if metadata.is_dir() {
                if stack.len() < 128 {
                    stack.push(path);
                }
                continue;
            }
            if scanned_files >= 500 {
                return;
            }
            if metadata.len() > 2 * 1024 * 1024 || !is_external_chat_file(&path) {
                continue;
            }
            scanned_files += 1;
            collect_external_skill_usage_from_file(app, &path, skill_names, out);
        }
    }
}

fn collect_external_skill_usage_from_file(
    app: &str,
    path: &Path,
    skill_names: &[String],
    out: &mut Vec<ExternalSkillUsageSample>,
) {
    let Ok(file) = File::open(path) else { return };
    let mut content = String::new();
    if file.take(2 * 1024 * 1024).read_to_string(&mut content).is_err() {
        return;
    }
    let lowered = content.to_lowercase();
    let activated_at = file_timestamp(path);
    let session_id = format!("external:{}:{}", app, stable_short_hash(&path.to_string_lossy()));
    for skill_name in skill_names {
        let count = skill_name_match_count(&lowered, skill_name);
        if count > 0 {
            out.push(ExternalSkillUsageSample {
                app: app.to_string(),
                skill_name: skill_name.clone(),
                activated_at: activated_at.clone(),
                session_id: session_id.clone(),
                count,
            });
        }
    }
}

fn external_chat_roots(app: &str) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = dirs::home_dir() {
        match app {
            "claude" => {
                roots.push(home.join(".claude").join("projects"));
                roots.push(home.join(".claude").join("chats"));
                roots.push(home.join("AppData").join("Roaming").join("Claude"));
            }
            "codex" => {
                roots.push(home.join(".codex").join("sessions"));
                roots.push(home.join(".codex").join("history"));
            }
            "gemini" => {
                roots.push(home.join(".gemini"));
                roots.push(home.join("AppData").join("Roaming").join("Gemini"));
            }
            "opencode" => {
                roots.push(home.join(".opencode"));
                roots.push(home.join(".local").join("share").join("opencode"));
            }
            _ => {}
        }
    }
    roots
}

fn is_external_chat_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|value| value.to_str()).map(|value| value.to_ascii_lowercase()),
        Some(ext) if matches!(ext.as_str(), "json" | "jsonl" | "md" | "txt" | "log")
    )
}

fn skill_name_match_count(content: &str, skill_name: &str) -> u64 {
    let needle = skill_name.to_lowercase();
    if needle.len() < 3 {
        return 0;
    }
    content.matches(&needle).count().min(10) as u64
}

fn file_timestamp(path: &Path) -> String {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map(|modified| {
            let datetime: chrono::DateTime<chrono::Utc> = modified.into();
            datetime.to_rfc3339()
        })
        .unwrap_or_else(|_| chrono::Utc::now().to_rfc3339())
}

fn usage_date_key(timestamp: &str) -> String {
    timestamp.get(0..10).unwrap_or(timestamp).to_string()
}

fn newer_timestamp(left: Option<String>, right: Option<String>) -> Option<String> {
    match (left, right) {
        (Some(left), Some(right)) => Some(if right > left { right } else { left }),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn stable_short_hash(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    format!("{:02x}{:02x}{:02x}{:02x}", digest[0], digest[1], digest[2], digest[3])
}

fn validate_skill_dir_name(name: &str) -> Result<()> {
    if name.trim().is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name == "."
        || name == ".."
        || name.contains("..")
    {
        return Err(anyhow!("Invalid skill directory name: {name}"));
    }
    Ok(())
}

fn copy_skill_dir(source: &Path, target: &Path) -> Result<()> {
    let staging_parent = target
        .parent()
        .ok_or_else(|| anyhow!("Target skill directory must have a parent."))?;
    fs::create_dir_all(staging_parent)
        .with_context(|| format!("Cannot create staging parent: {}", staging_parent.display()))?;
    let staging = tempfile::tempdir_in(staging_parent).with_context(|| {
        format!(
            "Cannot create staging directory in {}",
            staging_parent.display()
        )
    })?;
    let staged_root = staging.path().join(
        target
            .file_name()
            .ok_or_else(|| anyhow!("Target skill directory must have a name."))?,
    );
    copy_dir_recursive(source, &staged_root)?;
    if !staged_root.join("SKILL.md").is_file() {
        return Err(anyhow!("Copied skill is missing SKILL.md after staging."));
    }
    if target.exists() {
        return Err(anyhow!("Target skill already exists: {}", target.display()));
    }
    fs::rename(&staged_root, target)
        .with_context(|| format!("Cannot move staged skill into {}", target.display()))?;
    Ok(())
}

fn replace_skill_dir(source: &Path, target: &Path, backup: &Path) -> Result<()> {
    let staging_parent = target
        .parent()
        .ok_or_else(|| anyhow!("Target skill directory must have a parent."))?;
    fs::create_dir_all(staging_parent)
        .with_context(|| format!("Cannot create staging parent: {}", staging_parent.display()))?;
    let staging = tempfile::tempdir_in(staging_parent).with_context(|| {
        format!(
            "Cannot create staging directory in {}",
            staging_parent.display()
        )
    })?;
    let staged_root = staging.path().join(
        target
            .file_name()
            .ok_or_else(|| anyhow!("Target skill directory must have a name."))?,
    );
    copy_dir_recursive(source, &staged_root)?;
    if !staged_root.join("SKILL.md").is_file() {
        return Err(anyhow!("Copied skill is missing SKILL.md after staging."));
    }
    if backup.exists() {
        return Err(anyhow!("Backup path already exists: {}", backup.display()));
    }
    fs::rename(target, backup).with_context(|| {
        format!(
            "Cannot move current skill {} to backup {}",
            target.display(),
            backup.display()
        )
    })?;
    if let Err(error) = fs::rename(&staged_root, target) {
        let _ = fs::rename(backup, target);
        return Err(anyhow!(error).context(format!(
            "Cannot move staged skill into {}; rolled back from backup if possible",
            target.display()
        )));
    }
    Ok(())
}

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target)
        .with_context(|| format!("Cannot create directory: {}", target.display()))?;
    for entry in fs::read_dir(source)
        .with_context(|| format!("Cannot read directory: {}", source.display()))?
    {
        let entry = entry
            .with_context(|| format!("Cannot read directory entry in {}", source.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("Cannot inspect file type: {}", path.display()))?;
        let dest = target.join(entry.file_name());
        if file_type.is_symlink() {
            continue;
        } else if file_type.is_dir() {
            copy_dir_recursive(&path, &dest)?;
        } else if file_type.is_file() {
            fs::copy(&path, &dest).with_context(|| {
                format!(
                    "Cannot copy skill file {} -> {}",
                    path.display(),
                    dest.display()
                )
            })?;
        }
    }
    Ok(())
}

fn find_skill_dirs_limited(root: &Path, max_depth: usize) -> Vec<PathBuf> {
    fn visit(dir: &Path, depth: usize, max_depth: usize, out: &mut Vec<PathBuf>) {
        if dir.join("SKILL.md").is_file() {
            out.push(dir.to_path_buf());
            return;
        }
        if depth >= max_depth {
            return;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                visit(&entry.path(), depth + 1, max_depth, out);
            }
        }
    }
    let mut out = Vec::new();
    visit(root, 0, max_depth, &mut out);
    out
}

fn read_skill_description(skill_dir: &Path) -> Option<String> {
    let content = fs::read_to_string(skill_dir.join("SKILL.md")).ok()?;
    content
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("description:")
                .map(|v| v.trim().trim_matches('"').to_string())
        })
        .filter(|s| !s.is_empty())
}

fn skill_content_differs(source_dir: &Path, installed_dir: &Path) -> bool {
    let source = fs::read(source_dir.join("SKILL.md"));
    let installed = fs::read(installed_dir.join("SKILL.md"));
    match (source, installed) {
        (Ok(source), Ok(installed)) => source != installed,
        _ => false,
    }
}

fn skill_file_sha256(skill_dir: &Path) -> Option<String> {
    let bytes = fs::read(skill_dir.join("SKILL.md")).ok()?;
    let digest = Sha256::digest(bytes);
    Some(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn compare_market_skill(
    installed: bool,
    installed_version: Option<&str>,
    market_version: Option<&str>,
    installed_hash: Option<&str>,
    market_hash: Option<&str>,
) -> (bool, String, Option<String>) {
    if !installed {
        return (false, "notInstalled".to_string(), None);
    }

    if let (Some(local), Some(remote)) = (installed_version, market_version) {
        let local = local.trim();
        let remote = remote.trim();
        if !local.is_empty() && !remote.is_empty() {
            return if local != remote {
                (
                    true,
                    "version".to_string(),
                    Some(format!(
                        "local version {local} differs from market version {remote}"
                    )),
                )
            } else {
                (false, "version".to_string(), None)
            };
        }
    }

    if let (Some(local), Some(remote)) = (installed_hash, market_hash) {
        let local = local.trim().to_ascii_lowercase();
        let remote = remote.trim().to_ascii_lowercase();
        if !local.is_empty() && !remote.is_empty() {
            return if local != remote {
                (
                    true,
                    "hash".to_string(),
                    Some("local SKILL.md hash differs from market computed hash".to_string()),
                )
            } else {
                (false, "hash".to_string(), None)
            };
        }
    }

    (false, "unavailable".to_string(), None)
}

pub async fn install_remote_market_skill(
    request: SkillRemoteMarketInstallRequest,
) -> Result<SkillRemoteMarketInstallReport> {
    install_or_update_remote_market_skill(request, false).await
}

pub async fn update_remote_market_skill(
    request: SkillRemoteMarketInstallRequest,
) -> Result<SkillRemoteMarketInstallReport> {
    install_or_update_remote_market_skill(request, true).await
}

async fn install_or_update_remote_market_skill(
    request: SkillRemoteMarketInstallRequest,
    update: bool,
) -> Result<SkillRemoteMarketInstallReport> {
    validate_skill_dir_name(&request.name)?;
    if request.source_type != "github" {
        return Err(anyhow!(
            "Only GitHub skill market entries can be installed."
        ));
    }
    validate_github_repo(&request.source)?;
    validate_market_skill_path(&request.skill_path)?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(Policy::none())
        .user_agent("HopeAgent/skill-market-install")
        .build()
        .context("Cannot create skill market install HTTP client")?;
    let archive_url = format!("https://codeload.github.com/{}/tar.gz/HEAD", request.source);
    check_url(&archive_url, SsrfPolicy::Strict, &[]).await?;
    let response = client
        .get(&archive_url)
        .send()
        .await
        .with_context(|| format!("Cannot download GitHub archive for {}", request.source))?;
    if !response.status().is_success() {
        return Err(anyhow!(
            "GitHub archive download for {} returned HTTP {}.",
            request.source,
            response.status()
        ));
    }
    if response.content_length().unwrap_or(0) > REMOTE_SKILL_ARCHIVE_MAX_BYTES {
        return Err(anyhow!("Remote skill archive is too large."));
    }
    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("Cannot read GitHub archive for {}", request.source))?;
    if bytes.len() as u64 > REMOTE_SKILL_ARCHIVE_MAX_BYTES {
        return Err(anyhow!("Remote skill archive is too large."));
    }

    let target_root = crate::paths::skills_dir()?;
    fs::create_dir_all(&target_root)
        .with_context(|| format!("Cannot create skills directory: {}", target_root.display()))?;
    let root = target_root
        .canonicalize()
        .with_context(|| "Cannot access managed skills directory.".to_string())?;
    let target = root.join(&request.name);

    let extract_temp = tempfile::tempdir().context("Cannot create remote skill extraction dir")?;
    extract_github_archive_safely(&bytes, extract_temp.path())?;
    let unpacked_root = find_single_extracted_root(extract_temp.path())?;
    let source_dir = unpacked_root.join(request.skill_path.replace('\\', "/"));
    if !source_dir.is_dir() || !source_dir.join("SKILL.md").is_file() {
        return Err(anyhow!(
            "Remote skill path '{}' in {} does not contain SKILL.md.",
            request.skill_path,
            request.source
        ));
    }

    let verified_hash = skill_file_sha256(&source_dir);
    if let (Some(expected), Some(actual)) = (&request.market_hash, &verified_hash) {
        if !expected.trim().is_empty()
            && expected.trim().to_ascii_lowercase() != actual.trim().to_ascii_lowercase()
        {
            return Err(anyhow!(
                "Remote skill hash mismatch for '{}': expected {}, got {}.",
                request.name,
                expected,
                actual
            ));
        }
    }

    let backup_path = if update {
        let canonical_target = target.canonicalize().with_context(|| {
            format!(
                "Managed skill '{}' does not exist; install it before updating.",
                request.name
            )
        })?;
        if !canonical_target.starts_with(&root) || !canonical_target.join("SKILL.md").is_file() {
            return Err(anyhow!(
                "Refusing to update '{}': target is not a managed skill directory.",
                request.name
            ));
        }
        if !skill_content_differs(&source_dir, &canonical_target) {
            write_remote_market_metadata(&canonical_target, &request, verified_hash.clone())?;
            return Ok(SkillRemoteMarketInstallReport {
                name: request.name,
                source: request.source,
                skill_path: request.skill_path,
                target_path: canonical_target.to_string_lossy().to_string(),
                installed: true,
                updated: false,
                verified_hash,
                backup_path: None,
            });
        }
        let backup = root.join(format!(
            ".{}.backup.{}",
            request.name,
            chrono::Utc::now().format("%Y%m%d%H%M%S")
        ));
        replace_skill_dir(&source_dir, &canonical_target, &backup)?;
        write_remote_market_metadata(&canonical_target, &request, verified_hash.clone())?;
        Some(backup.to_string_lossy().to_string())
    } else {
        if target.exists() {
            return Err(anyhow!(
                "Skill '{}' already exists at {}; use update instead.",
                request.name,
                target.display()
            ));
        }
        copy_skill_dir(&source_dir, &target)?;
        write_remote_market_metadata(&target, &request, verified_hash.clone())?;
        None
    };
    bump_skill_version();
    Ok(SkillRemoteMarketInstallReport {
        name: request.name,
        source: request.source,
        skill_path: request.skill_path,
        target_path: target.to_string_lossy().to_string(),
        installed: true,
        updated: update,
        verified_hash,
        backup_path,
    })
}

fn validate_github_repo(source: &str) -> Result<()> {
    let parts: Vec<&str> = source.split('/').collect();
    if parts.len() != 2
        || parts.iter().any(|part| {
            part.is_empty()
                || part == &"."
                || part == &".."
                || !part
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')
        })
    {
        return Err(anyhow!("Invalid GitHub repository source: {source}"));
    }
    Ok(())
}

fn validate_market_skill_path(path: &str) -> Result<()> {
    let normalized = path.replace('\\', "/");
    if normalized.trim().is_empty()
        || normalized.starts_with('/')
        || normalized.contains(":")
        || normalized
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(anyhow!("Invalid remote skill path: {path}"));
    }
    Ok(())
}

fn extract_github_archive_safely(bytes: &[u8], target: &Path) -> Result<()> {
    let decoder = flate2::read::GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);
    let mut entry_count = 0usize;
    let mut total_size = 0u64;
    for entry in archive
        .entries()
        .context("Cannot read remote skill archive")?
    {
        let mut entry = entry.context("Cannot inspect remote skill archive entry")?;
        let entry_type = entry.header().entry_type();
        if !(entry_type.is_file() || entry_type.is_dir()) {
            continue;
        }
        entry_count += 1;
        if entry_count > REMOTE_SKILL_EXTRACT_MAX_ENTRIES {
            return Err(anyhow!("Remote skill archive has too many entries."));
        }
        total_size = total_size.saturating_add(entry.header().size().unwrap_or(0));
        if total_size > REMOTE_SKILL_EXTRACT_MAX_BYTES {
            return Err(anyhow!(
                "Remote skill archive uncompressed size is too large."
            ));
        }
        let path = entry
            .path()
            .context("Cannot read remote skill archive path")?;
        if path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        }) {
            return Err(anyhow!("Remote skill archive contains an unsafe path."));
        }
        entry.unpack_in(target).with_context(|| {
            format!(
                "Cannot unpack remote skill archive into {}",
                target.display()
            )
        })?;
    }
    Ok(())
}

fn find_single_extracted_root(root: &Path) -> Result<PathBuf> {
    let dirs: Vec<PathBuf> = fs::read_dir(root)
        .with_context(|| format!("Cannot read extraction dir: {}", root.display()))?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|file_type| file_type.is_dir())
                .map(|_| entry.path())
        })
        .collect();
    if dirs.len() != 1 {
        return Err(anyhow!(
            "Remote skill archive must extract to exactly one root directory."
        ));
    }
    Ok(dirs[0].clone())
}

fn write_remote_market_metadata(
    target: &Path,
    request: &SkillRemoteMarketInstallRequest,
    verified_hash: Option<String>,
) -> Result<()> {
    let metadata = SkillRemoteMarketInstalledMetadata {
        name: request.name.clone(),
        source: request.source.clone(),
        source_type: request.source_type.clone(),
        skill_path: request.skill_path.clone(),
        source_id: request.source_id.clone(),
        source_name: request.source_name.clone(),
        market_hash: request.market_hash.clone().or(verified_hash),
        market_version: request.market_version.clone(),
        installed_at: chrono::Utc::now().to_rfc3339(),
    };
    let content = serde_json::to_string_pretty(&metadata)
        .context("Cannot serialize skill market metadata")?;
    fs::write(target.join(".hope-skill-market.json"), content).with_context(|| {
        format!(
            "Cannot write skill market metadata for {}",
            target.display()
        )
    })?;
    Ok(())
}

fn external_app_skills_root(app: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    match app {
        "claude" => Some(home.join(".claude").join("skills")),
        "codex" => Some(home.join(".codex").join("skills")),
        "gemini" => Some(home.join(".gemini").join("skills")),
        "opencode" => Some(home.join(".opencode").join("skills")),
        "hope" => crate::paths::skills_dir().ok(),
        _ => None,
    }
}

// ── Catalog / detail ──────────────────────────────────────────────

pub fn list_skills() -> Vec<SkillSummary> {
    let store = crate::config::cached_config();
    let entries = load_all_skills_with_budget(&store.extra_skills_dirs, &store.skill_prompt_budget);
    let disabled = &store.disabled_skills;
    entries
        .into_iter()
        .map(|e| {
            let enabled = !disabled.contains(&e.name);
            e.to_summary(enabled)
        })
        .collect()
}

pub fn reload_skills() -> Vec<SkillSummary> {
    bump_skill_version();
    list_skills()
}

pub fn get_skill_detail(name: &str) -> Option<SkillDetail> {
    let store = crate::config::cached_config();
    get_skill_content(name, &store.extra_skills_dirs, &store.disabled_skills)
}

// ── Extra skills directories ──────────────────────────────────────

pub fn get_extra_skills_dirs() -> Vec<String> {
    crate::config::cached_config().extra_skills_dirs.clone()
}

pub fn add_extra_skills_dir(dir: String, source: &str) -> Result<()> {
    crate::config::mutate_config(("extra_skills_dirs", source), |store| {
        if !store.extra_skills_dirs.contains(&dir) {
            store.extra_skills_dirs.push(dir);
        }
        Ok(())
    })?;
    bump_skill_version();
    Ok(())
}

pub fn remove_extra_skills_dir(dir: &str, source: &str) -> Result<()> {
    crate::config::mutate_config(("extra_skills_dirs", source), |store| {
        store.extra_skills_dirs.retain(|d| d != dir);
        Ok(())
    })?;
    bump_skill_version();
    Ok(())
}

// ── Enable / disable ──────────────────────────────────────────────

pub fn toggle_skill(name: String, enabled: bool, source: &str) -> Result<()> {
    crate::config::mutate_config(("disabled_skills", source), |store| {
        if enabled {
            store.disabled_skills.retain(|n| n != &name);
        } else if !store.disabled_skills.contains(&name) {
            store.disabled_skills.push(name);
        }
        Ok(())
    })?;
    bump_skill_version();
    Ok(())
}

// ── Skill env-check + per-skill env vars ──────────────────────────

pub fn get_skill_env_check() -> bool {
    crate::config::cached_config().skill_env_check
}

pub fn set_skill_env_check(enabled: bool, source: &str) -> Result<()> {
    crate::config::mutate_config(("skill_env_check", source), |store| {
        store.skill_env_check = enabled;
        Ok(())
    })?;
    bump_skill_version();
    Ok(())
}

/// Env vars for a skill with values masked (safe to return to UI).
pub fn get_skill_env_masked(name: &str) -> HashMap<String, String> {
    crate::config::cached_config()
        .skill_env
        .get(name)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|(k, v)| (k, mask_value(&v)))
        .collect()
}

/// Set one env var for a skill. Returns Ok(()) without writing when `value`
/// is the masked placeholder — prevents the UI from accidentally replacing a
/// real secret with its own mask.
pub fn set_skill_env_var(skill: String, key: String, value: String, source: &str) -> Result<()> {
    if is_masked_value(&value) {
        return Ok(());
    }
    crate::config::mutate_config(("skill_env", source), |store| {
        store.skill_env.entry(skill).or_default().insert(key, value);
        Ok(())
    })?;
    bump_skill_version();
    Ok(())
}

pub fn remove_skill_env_var(skill: &str, key: &str, source: &str) -> Result<()> {
    crate::config::mutate_config(("skill_env", source), |store| {
        if let Some(map) = store.skill_env.get_mut(skill) {
            map.remove(key);
            if map.is_empty() {
                store.skill_env.remove(skill);
            }
        }
        Ok(())
    })?;
    bump_skill_version();
    Ok(())
}

/// `skill → { env_var → configured? }` snapshot (configured = user-set or
/// inherited from the process environment). Only skills that declare
/// `requires.env` are included.
pub fn get_skills_env_status() -> HashMap<String, HashMap<String, bool>> {
    let store = crate::config::cached_config();
    let entries = load_all_skills_with_budget(&store.extra_skills_dirs, &store.skill_prompt_budget);
    let mut result = HashMap::new();
    for entry in &entries {
        if entry.requires.env.is_empty() {
            continue;
        }
        let configured = store.skill_env.get(&entry.name);
        let mut status = HashMap::new();
        for key in &entry.requires.env {
            let has_configured = configured
                .and_then(|m| m.get(key))
                .map(|v| !v.is_empty())
                .unwrap_or(false);
            let has_system = std::env::var(key).map(|v| !v.is_empty()).unwrap_or(false);
            status.insert(key.clone(), has_configured || has_system);
        }
        result.insert(entry.name.clone(), status);
    }
    result
}

pub fn get_skills_status() -> Vec<SkillStatusEntry> {
    let store = crate::config::cached_config();
    let entries = load_all_skills_with_budget(&store.extra_skills_dirs, &store.skill_prompt_budget);
    check_all_skills_status(
        &entries,
        &store.disabled_skills,
        store.skill_env_check,
        &store.skill_env,
        &store.skill_allow_bundled,
    )
}

// ── Phase B' draft review ─────────────────────────────────────────

pub fn list_draft_skills() -> Vec<SkillSummary> {
    let store = crate::config::cached_config();
    let drafts = author::list_drafts(&store.extra_skills_dirs);
    let disabled = &store.disabled_skills;
    drafts
        .into_iter()
        .map(|e| {
            let enabled = !disabled.contains(&e.name);
            e.to_summary(enabled)
        })
        .collect()
}

pub fn activate_draft_skill(name: &str) -> Result<()> {
    author::set_skill_status(name, SkillStatus::Active)
}

pub fn discard_draft_skill(name: &str) -> Result<()> {
    author::delete_skill(name)
}

/// Read the current auto-review promotion mode.
/// `true` = newly auto-created skills are written directly as `Active` (skip review).
/// `false` = newly auto-created skills land in `Draft` for manual user activation.
pub fn get_auto_review_promotion() -> bool {
    matches!(
        crate::config::cached_config().skills.auto_review.promotion,
        auto_review::AutoReviewPromotion::Auto
    )
}

/// Toggle the auto-review promotion mode. `true` skips the draft buffer.
pub fn set_auto_review_promotion(auto: bool, source: &str) -> Result<()> {
    crate::config::mutate_config(("skills.auto_review", source), |store| {
        store.skills.auto_review.promotion = if auto {
            auto_review::AutoReviewPromotion::Auto
        } else {
            auto_review::AutoReviewPromotion::Draft
        };
        Ok(())
    })?;
    Ok(())
}

/// Read the master enabled flag for the auto-review pipeline.
/// `true` (default) = post-turn cooldown + threshold gating runs and may invoke
/// the side_query review. `false` = the pipeline is fully suppressed; nothing
/// auto-creates or auto-patches skills.
pub fn get_auto_review_enabled() -> bool {
    crate::config::cached_config().skills.auto_review.enabled
}

/// Toggle the master enabled flag.
pub fn set_auto_review_enabled(enabled: bool, source: &str) -> Result<()> {
    crate::config::mutate_config(("skills.auto_review", source), |store| {
        store.skills.auto_review.enabled = enabled;
        Ok(())
    })?;
    Ok(())
}

// ── Full-config read / patch / reset (Settings panel) ─────────────────

/// Snapshot of the sanitized auto-review config as a JSON-ready value.
/// Used by the Settings panel; UI binds to camelCase keys directly.
pub fn get_auto_review_config_snapshot() -> auto_review::SkillsAutoReviewConfig {
    crate::config::cached_config()
        .skills
        .auto_review
        .clone()
        .sanitize()
}

/// Deep-merge a JSON `patch` object into the live auto-review config,
/// re-sanitize, and persist. Unknown keys in the patch are ignored
/// by the serde round-trip (no strict-mode), letting the API stay
/// forward-compatible with future fields.
pub fn set_auto_review_config_patch(
    patch: serde_json::Value,
    source: &str,
) -> Result<auto_review::SkillsAutoReviewConfig> {
    if !patch.is_object() {
        anyhow::bail!("auto_review patch must be a JSON object");
    }
    crate::config::mutate_config(("skills.auto_review", source), |store| {
        let mut current = serde_json::to_value(&store.skills.auto_review)
            .context("serialize current auto_review config")?;
        crate::util::merge_json(&mut current, patch.clone());
        let next: auto_review::SkillsAutoReviewConfig =
            serde_json::from_value(current).context("deserialize merged auto_review config")?;
        store.skills.auto_review = next.sanitize();
        Ok(())
    })?;
    Ok(get_auto_review_config_snapshot())
}

/// Reset specific snake_case fields to their built-in defaults (or every
/// field when `fields` is `None`). Unknown field names are silently
/// ignored — the API layer can validate up front if it cares.
pub fn reset_auto_review_config(
    fields: Option<Vec<String>>,
    source: &str,
) -> Result<auto_review::SkillsAutoReviewConfig> {
    crate::config::mutate_config(("skills.auto_review", source), |store| {
        store.skills.auto_review.reset_fields(fields.as_deref());
        store.skills.auto_review = store.skills.auto_review.clone().sanitize();
        Ok(())
    })?;
    Ok(get_auto_review_config_snapshot())
}

/// Pull the most recent `skill_review_skipped` rows so the Settings UI
/// can render a "why didn't this run produce a draft?" timeline.
/// Returns JSON tuples `{ ts, sessionId, skillId?, rejectReason?,
/// rationale?, fireReason? }`, most recent first.
///
/// Reads the raw event timeline (no dedup, no `ref_id IS NOT NULL`
/// filter) — most skip events carry no `skill_id` at all because the
/// gate fired before a candidate had an id, so a deduped view would
/// drop the most common cases.
pub fn recent_auto_review_skips(limit: usize) -> Vec<serde_json::Value> {
    let Some(db) = crate::get_session_db() else {
        return Vec::new();
    };
    // 7-day window matches what we surface in the dashboard; users
    // configuring a longer retention can still see further back via
    // direct DB inspection.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let since = now.saturating_sub(7 * 86_400);
    let rows = db
        .recent_learning_events_timeline(auto_review::EVT_SKILL_REVIEW_SKIPPED, since, limit.max(1))
        .unwrap_or_default();
    rows.into_iter()
        .map(|(ts, ref_id, session_id, meta_json)| {
            let parsed = meta_json
                .as_deref()
                .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok());
            let mut out = serde_json::Map::new();
            out.insert("ts".into(), serde_json::Value::Number(ts.into()));
            out.insert(
                "skillId".into(),
                ref_id
                    .filter(|s| !s.is_empty())
                    .map(serde_json::Value::String)
                    .unwrap_or(serde_json::Value::Null),
            );
            out.insert(
                "sessionId".into(),
                session_id
                    .filter(|s| !s.is_empty())
                    .map(serde_json::Value::String)
                    .unwrap_or(serde_json::Value::Null),
            );
            if let Some(serde_json::Value::Object(m)) = parsed {
                if let Some(v) = m.get("reject_reason") {
                    out.insert("rejectReason".into(), v.clone());
                }
                if let Some(v) = m.get("rationale") {
                    out.insert("rationale".into(), v.clone());
                }
                if let Some(v) = m.get("fire_reason") {
                    out.insert("fireReason".into(), v.clone());
                }
                // Prefer the column-level `session_id` written by
                // `record_learning_event` over a duplicate inside
                // `meta_json`, but fall back to it when present.
                if !out.contains_key("sessionId") || out["sessionId"].is_null() {
                    if let Some(v) = m.get("session_id") {
                        out.insert("sessionId".into(), v.clone());
                    }
                }
            }
            serde_json::Value::Object(out)
        })
        .collect()
}

// ── Curator (draft consolidation) ───────────────────────────────────

/// Synchronous scan that surfaces merge proposals for clusters of
/// near-duplicate draft skills. Safe to call from a Tauri command or
/// HTTP route directly — no LLM, no disk writes.
pub fn run_curator_pass_sync() -> Result<auto_review::curator::CuratorReport> {
    auto_review::curator::run_curator_pass()
}

/// Apply a curator merge proposal: keep `keep_id`, delete the rest.
/// Returns the number of drafts actually discarded.
pub fn apply_curator_merge(keep_id: &str, member_ids: &[String]) -> Result<usize> {
    auto_review::curator::apply_merge_keep_id(keep_id, member_ids)
}

// ── Install dependency ────────────────────────────────────────────
//
// Spawns a package-manager process (`brew install …`, `npm install -g …`,
// `go install …`, `uv tool install …`) based on the skill's `install:` spec.
//
// SECURITY: the core function itself performs no authorization — callers
// decide whether the request is trusted:
//   * Tauri desktop: unconditional (user clicked in their own GUI = intent).
//   * HTTP surface: gate on `AppConfig.skills.allow_remote_install` — an
//     opt-in flag that must be flipped manually in settings. Without it,
//     anyone with the API key could pivot to arbitrary package installs.

/// Run the install spec at `spec_index` for `skill_name`. Returns combined
/// `stdout + stderr + binary verification` log on success, or an error with
/// the same format when the process exits non-zero.
pub async fn install_skill_dependency(skill_name: &str, spec_index: usize) -> Result<String> {
    let (cmd_program, cmd_args, bins) = {
        let store = crate::config::cached_config();
        let entries =
            load_all_skills_with_budget(&store.extra_skills_dirs, &store.skill_prompt_budget);
        let skill = entries
            .into_iter()
            .find(|s| s.name == skill_name)
            .ok_or_else(|| anyhow!("Skill not found: {}", skill_name))?;

        let spec = skill
            .install
            .get(spec_index)
            .ok_or_else(|| anyhow!("Install spec index {} out of range", spec_index))?
            .clone();

        // OS guard — refuse to spawn platform-mismatched installers so the
        // user doesn't hit a cryptic process-spawn failure.
        if !spec.os.is_empty() {
            let current = std::env::consts::OS;
            let ok = spec.os.iter().any(|os| {
                os == current
                    || (os == "darwin" && current == "macos")
                    || (os == "mac" && current == "macos")
            });
            if !ok {
                return Err(anyhow!(
                    "Install spec is not available on this platform ({}), requires: {:?}",
                    current,
                    spec.os
                ));
            }
        }

        match spec.kind.as_str() {
            "brew" => {
                let formula = spec
                    .formula
                    .as_deref()
                    .ok_or_else(|| anyhow!("Brew install spec missing 'formula' field"))?;
                // Reject flag-looking / traversal args so we never feed the
                // spec into brew as an option flag.
                if formula.contains("..") || formula.contains('\\') || formula.starts_with('-') {
                    return Err(anyhow!("Invalid brew formula name"));
                }
                (
                    "brew".to_string(),
                    vec!["install".to_string(), formula.to_string()],
                    spec.bins,
                )
            }
            "node" => {
                let package = spec
                    .package
                    .as_deref()
                    .ok_or_else(|| anyhow!("Node install spec missing 'package' field"))?;
                if package.contains("..") || package.contains('\\') {
                    return Err(anyhow!("Invalid npm package name"));
                }
                (
                    "npm".to_string(),
                    vec!["install".to_string(), "-g".to_string(), package.to_string()],
                    spec.bins,
                )
            }
            "go" => {
                let module = spec
                    .go_module
                    .as_deref()
                    .ok_or_else(|| anyhow!("Go install spec missing 'module' field"))?;
                if module.contains("..") || module.contains('\\') {
                    return Err(anyhow!("Invalid go module path"));
                }
                (
                    "go".to_string(),
                    vec!["install".to_string(), module.to_string()],
                    spec.bins,
                )
            }
            "uv" => {
                let package = spec
                    .package
                    .as_deref()
                    .ok_or_else(|| anyhow!("UV install spec missing 'package' field"))?;
                (
                    "uv".to_string(),
                    vec![
                        "tool".to_string(),
                        "install".to_string(),
                        package.to_string(),
                    ],
                    spec.bins,
                )
            }
            other => return Err(anyhow!("Unsupported install kind: {}", other)),
        }
    };

    let args_ref: Vec<&str> = cmd_args.iter().map(String::as_str).collect();
    let output = run_install_command(&cmd_program, &args_ref).await?;

    let mut verification = String::new();
    for bin in &bins {
        if binary_in_path_public(bin) {
            verification.push_str(&format!("\n✓ {} found in PATH", bin));
        } else {
            verification.push_str(&format!("\n✗ {} not found in PATH", bin));
        }
    }

    bump_skill_version();
    Ok(format!("{}{}", output, verification))
}

async fn run_install_command(program: &str, args: &[&str]) -> Result<String> {
    let mut cmd = tokio::process::Command::new(program);
    crate::platform::hide_console_tokio(&mut cmd);
    cmd.args(args);
    crate::platform::hide_console_tokio(&mut cmd);
    let output = cmd
        .output()
        .await
        .map_err(|e| anyhow!("Failed to run {} {}: {}", program, args.join(" "), e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.success() {
        Ok(format!("{}{}", stdout, stderr))
    } else {
        Err(anyhow!(
            "{} {} failed (exit code {:?}):\n{}\n{}",
            program,
            args.join(" "),
            output.status.code(),
            stdout,
            stderr
        ))
    }
}

// ── Quick Import: preset skill source discovery ───────────────────
//
// Probes a small set of known third-party skill catalogs on the user's
// machine (Claude Code user-level + plugins, Anthropic agent-skills
// marketplace, OpenClaw and Hermes Agent clones) and returns a structured
// listing for the Settings / Onboarding "Quick Import" UI. Adding the
// returned paths to `extra_skills_dirs` is done with the existing
// `add_extra_skills_dir` flow — this RPC is read-only.

/// One discoverable skill catalog source. Always returned even when none of
/// its candidate paths exist, so the UI can render a complete row with the
/// expected paths grayed out.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetSkillSource {
    /// Stable id, used as the i18n lookup key suffix (e.g. "claude-code-user").
    pub id: String,
    /// i18n key for the human-readable label.
    pub label_key: String,
    /// Optional warning key — the UI surfaces a `⚠️` badge with this i18n
    /// string when present (e.g. OpenClaw "skills depend on external CLIs",
    /// Anthropic marketplace "Proprietary license").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning_key: Option<String>,
    /// Concrete candidate paths probed for this source. Multiple paths can
    /// map to one logical source (Claude Code plugins scan two layouts;
    /// OW/HA repos may live in `~/Codes`, `~/git`, etc.).
    pub candidates: Vec<PresetCandidate>,
}

/// One concrete filesystem candidate inside a [`PresetSkillSource`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetCandidate {
    /// Absolute path. `~` is resolved on the server, not the browser.
    pub path: String,
    /// Whether the directory exists right now.
    pub exists: bool,
    /// Number of `SKILL.md`-bearing subdirectories found (recursive ≤ 2 levels).
    /// `0` when the path doesn't exist or contains no skills — the UI shows
    /// the row anyway so the user knows what was probed.
    pub skill_count: usize,
    /// True when this exact path is already in `extra_skills_dirs`.
    pub already_added: bool,
}

/// Probe known third-party skill catalog locations. Returns 5 sources in a
/// stable order regardless of what's installed locally — the UI renders
/// "not found" rows for absent ones.
pub fn discover_preset_skill_sources() -> Vec<PresetSkillSource> {
    let store = crate::config::cached_config();
    let extra: Vec<String> = store.extra_skills_dirs.clone();
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => {
            // No home → return all sources with empty candidate lists.
            return preset_sources_layout()
                .into_iter()
                .map(|(id, label, warning, _candidates)| PresetSkillSource {
                    id: id.to_string(),
                    label_key: label.to_string(),
                    warning_key: warning.map(|w| w.to_string()),
                    candidates: vec![],
                })
                .collect();
        }
    };

    let mut out = Vec::new();
    for (id, label, warning, candidate_layouts) in preset_sources_layout() {
        let mut candidates = Vec::new();
        for layout in candidate_layouts {
            let paths = layout.expand(&home);
            for path in paths {
                let exists = path.is_dir();
                let skill_count = if exists {
                    count_skills_in_dir(&path, 2)
                } else {
                    0
                };
                let path_str = path.to_string_lossy().to_string();
                let already_added = extra.iter().any(|d| d == &path_str);
                candidates.push(PresetCandidate {
                    path: path_str,
                    exists,
                    skill_count,
                    already_added,
                });
            }
        }
        // Deduplicate by path while preserving order.
        let mut seen = std::collections::HashSet::new();
        candidates.retain(|c| seen.insert(c.path.clone()));
        out.push(PresetSkillSource {
            id: id.to_string(),
            label_key: label.to_string(),
            warning_key: warning.map(|w| w.to_string()),
            candidates,
        });
    }
    out
}

/// Layout describing how to compute one or more paths under a user's HOME.
/// Either fixed (one literal path) or globbed (e.g. `~/.claude/plugins/*/skills`).
enum CandidateLayout {
    Fixed(&'static str),
    Glob {
        prefix: &'static str,
        suffix: &'static str,
    },
}

impl CandidateLayout {
    fn expand(&self, home: &Path) -> Vec<PathBuf> {
        match self {
            CandidateLayout::Fixed(rel) => vec![home.join(rel)],
            CandidateLayout::Glob { prefix, suffix } => {
                let parent = home.join(prefix);
                let entries = match std::fs::read_dir(&parent) {
                    Ok(rd) => rd,
                    Err(_) => return vec![],
                };
                let mut out = Vec::new();
                for entry in entries.flatten() {
                    if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                        let candidate = entry.path().join(suffix);
                        if candidate.is_dir() {
                            out.push(candidate);
                        }
                    }
                }
                out
            }
        }
    }
}

#[allow(clippy::type_complexity)]
fn preset_sources_layout() -> Vec<(
    &'static str,
    &'static str,
    Option<&'static str>,
    Vec<CandidateLayout>,
)> {
    vec![
        (
            "claude-code-user",
            "settings.skillsImport.cc.user",
            None,
            vec![CandidateLayout::Fixed(".claude/skills")],
        ),
        (
            "claude-code-plugins",
            "settings.skillsImport.cc.plugins",
            None,
            vec![
                CandidateLayout::Glob {
                    prefix: ".claude/plugins",
                    suffix: "skills",
                },
                CandidateLayout::Glob {
                    prefix: ".claude/plugins",
                    suffix: ".claude-plugin/skills",
                },
            ],
        ),
        (
            "anthropic-marketplace",
            "settings.skillsImport.cc.anthropic",
            Some("settings.skillsImport.warning.proprietary"),
            vec![CandidateLayout::Fixed(
                ".claude/plugins/marketplaces/anthropic-agent-skills/skills",
            )],
        ),
        (
            "openclaw",
            "settings.skillsImport.openclaw",
            Some("settings.skillsImport.warning.openclaw"),
            vec![
                CandidateLayout::Fixed("Codes/openclaw/skills"),
                CandidateLayout::Fixed("git/openclaw/skills"),
                CandidateLayout::Fixed("projects/openclaw/skills"),
                CandidateLayout::Fixed("openclaw/skills"),
            ],
        ),
        (
            "hermes-agent",
            "settings.skillsImport.hermes",
            None,
            vec![
                CandidateLayout::Fixed("Codes/hermes-agent/skills"),
                CandidateLayout::Fixed("git/hermes-agent/skills"),
                CandidateLayout::Fixed("projects/hermes-agent/skills"),
                CandidateLayout::Fixed("hermes-agent/skills"),
            ],
        ),
    ]
}

/// Count subdirectories containing a `SKILL.md`, recursing up to `max_depth`
/// levels deep (Hermes Agent groups skills in two-level category trees, so
/// depth 2 covers it; one level is enough for everything else).
fn count_skills_in_dir(dir: &Path, max_depth: usize) -> usize {
    fn walk(dir: &Path, depth: usize, max_depth: usize, count: &mut usize, budget: &mut usize) {
        if *budget == 0 || depth > max_depth {
            return;
        }
        let entries = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            if *budget == 0 {
                return;
            }
            *budget -= 1;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if path.join("SKILL.md").is_file() {
                *count += 1;
                continue; // Don't recurse into a confirmed skill dir.
            }
            if depth < max_depth {
                walk(&path, depth + 1, max_depth, count, budget);
            }
        }
    }
    let mut count = 0usize;
    // Cap the walk to keep the discovery RPC snappy even when the user has
    // huge unrelated trees under the probed parents.
    let mut budget = 2000usize;
    walk(dir, 0, max_depth, &mut count, &mut budget);
    count
}

pub async fn trigger_skill_review_now(session_id: &str) -> Result<serde_json::Value> {
    let gate = auto_review::acquire_manual(session_id)
        .ok_or_else(|| anyhow::anyhow!("another review is already running for this session"))?;
    let report =
        auto_review::run_review_cycle(session_id, auto_review::ReviewTrigger::Manual, gate, None)
            .await?;
    Ok(serde_json::to_value(report)?)
}
