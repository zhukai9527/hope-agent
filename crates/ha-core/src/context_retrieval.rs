//! Context Retrieval v2: task-aware read-only context ranking for the
//! workspace panel.
//!
//! This module deliberately aggregates existing owner-plane signals instead of
//! creating another mutable control-plane object. It ranks the files, semantic
//! diagnostics, review findings, verification steps, symbols, and URL sources a
//! user is most likely to need next. Incognito sessions return an empty,
//! explicitly-disabled snapshot.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use crate::review::{ReviewFindingStatus, ReviewSeverity};
use crate::session::{effective_working_dir_for_meta, SessionDB};
use crate::util::now_rfc3339;
use crate::verification::VerificationStepState;

const DEFAULT_LIMIT: usize = 24;
const MAX_LIMIT: usize = 50;
const REVIEW_RUN_LIMIT: usize = 3;
const VERIFICATION_RUN_LIMIT: usize = 3;
const FILE_SEARCH_LIMIT: usize = 24;
const SYMBOL_SEARCH_LIMIT: usize = 24;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextRetrievalInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ContextCandidateKind {
    File,
    Symbol,
    Diagnostic,
    ReviewFinding,
    VerificationStep,
    UrlSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextCandidate {
    pub id: String,
    pub kind: ContextCandidateKind,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub score: u32,
    pub reasons: Vec<String>,
    pub sources: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextRetrievalStats {
    pub git_changes: usize,
    pub artifact_files: usize,
    pub diagnostics: usize,
    pub review_findings: usize,
    pub verification_steps: usize,
    pub file_search_matches: usize,
    pub symbols: usize,
    pub url_sources: usize,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextRetrievalSnapshot {
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    pub candidates: Vec<ContextCandidate>,
    pub stats: ContextRetrievalStats,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled_reason: Option<String>,
    pub generated_at: String,
}

struct CandidateAccum {
    candidate: ContextCandidate,
    rank: i32,
}

#[derive(Debug, Clone)]
struct QueryMatcher {
    raw: String,
    tokens: Vec<String>,
}

impl QueryMatcher {
    fn new(query: Option<&str>) -> Self {
        let raw = query.unwrap_or("").trim().to_lowercase();
        let tokens = raw
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        Self { raw, tokens }
    }

    fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    fn boost(&self, fields: &[&str]) -> i32 {
        if self.is_empty() {
            return 0;
        }
        let haystack = fields
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_lowercase())
            .collect::<Vec<_>>()
            .join("\n");
        if haystack.is_empty() {
            return 0;
        }
        let mut boost = 0;
        if haystack.contains(&self.raw) {
            boost += 260;
        }
        let mut matched = 0;
        for token in &self.tokens {
            if haystack.contains(token) {
                matched += 1;
                boost += 55;
            }
        }
        if matched > 0 && matched == self.tokens.len() {
            boost += 160;
        }
        boost
    }
}

pub async fn context_retrieval_for_session(
    db: Arc<SessionDB>,
    session_id: String,
    input: ContextRetrievalInput,
) -> Result<ContextRetrievalSnapshot> {
    let limit = input.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let query = input
        .query
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let matcher = QueryMatcher::new(query.as_deref());
    let meta = db
        .get_session(&session_id)?
        .ok_or_else(|| anyhow!("session not found: {session_id}"))?;
    let workspace_root = effective_working_dir_for_meta(&meta);

    if meta.incognito {
        return Ok(ContextRetrievalSnapshot {
            session_id,
            query,
            workspace_root,
            candidates: Vec::new(),
            stats: ContextRetrievalStats::default(),
            truncated: false,
            disabled_reason: Some("incognito".to_string()),
            generated_at: now_rfc3339(),
        });
    }

    let mut stats = ContextRetrievalStats::default();
    let mut map: HashMap<String, CandidateAccum> = HashMap::new();

    gather_git_changes(db.clone(), &session_id, &matcher, &mut map, &mut stats).await;
    gather_artifacts(db.clone(), &session_id, &matcher, &mut map, &mut stats).await;
    gather_lsp_diagnostics(&db, &session_id, &matcher, &mut map, &mut stats).await;
    gather_review_findings(&db, &session_id, &matcher, &mut map, &mut stats);
    gather_verification_steps(&db, &session_id, &matcher, &mut map, &mut stats);
    gather_file_search(
        workspace_root.as_deref(),
        query.as_deref(),
        &matcher,
        &mut map,
        &mut stats,
    )
    .await;
    gather_lsp_symbols(
        &db,
        &session_id,
        query.as_deref(),
        &matcher,
        &mut map,
        &mut stats,
    )
    .await;

    let mut candidates = map.into_values().collect::<Vec<_>>();
    candidates.sort_by(|a, b| {
        b.rank
            .cmp(&a.rank)
            .then_with(|| kind_rank(&a.candidate.kind).cmp(&kind_rank(&b.candidate.kind)))
            .then_with(|| a.candidate.title.cmp(&b.candidate.title))
    });
    let truncated = candidates.len() > limit;
    let candidates = candidates
        .into_iter()
        .take(limit)
        .map(|acc| acc.candidate)
        .collect();

    Ok(ContextRetrievalSnapshot {
        session_id,
        query,
        workspace_root,
        candidates,
        stats,
        truncated,
        disabled_reason: None,
        generated_at: now_rfc3339(),
    })
}

async fn gather_git_changes(
    db: Arc<SessionDB>,
    session_id: &str,
    matcher: &QueryMatcher,
    map: &mut HashMap<String, CandidateAccum>,
    stats: &mut ContextRetrievalStats,
) {
    let sid = session_id.to_string();
    let diff =
        tokio::task::spawn_blocking(move || crate::session::load_session_git_diff(&db, &sid)).await;
    let Ok(Ok(diff)) = diff else {
        return;
    };
    stats.git_changes = diff.changes.len();
    for change in diff.changes {
        let action = format!("{:?}", change.action).to_lowercase();
        let line_impact = (change.lines_added + change.lines_removed).min(200) as i32;
        let boost = matcher.boost(&[&change.path, change.language, &action]);
        upsert_candidate(
            map,
            format!("file:{}", change.path),
            ContextCandidate {
                id: format!("file:{}", change.path),
                kind: ContextCandidateKind::File,
                title: display_path(&change.path),
                subtitle: Some(change.path.clone()),
                path: Some(change.path.clone()),
                line: None,
                url: None,
                score: 0,
                reasons: Vec::new(),
                sources: Vec::new(),
                status: Some(action),
                metadata: json!({
                    "origin": "git_change",
                    "linesAdded": change.lines_added,
                    "linesRemoved": change.lines_removed,
                    "language": change.language,
                    "truncated": change.truncated,
                }),
            },
            900 + line_impact + boost,
            "当前 Git diff 修改过这个文件",
            "git",
        );
    }
}

async fn gather_artifacts(
    db: Arc<SessionDB>,
    session_id: &str,
    matcher: &QueryMatcher,
    map: &mut HashMap<String, CandidateAccum>,
    stats: &mut ContextRetrievalStats,
) {
    let sid = session_id.to_string();
    let artifacts =
        tokio::task::spawn_blocking(move || crate::session::aggregate_session_artifacts(&db, &sid))
            .await;
    let Ok(Ok(artifacts)) = artifacts else {
        return;
    };
    stats.artifact_files = artifacts.files.len();
    stats.url_sources = artifacts.sources.len();
    if artifacts.files_truncated {
        stats
            .warnings
            .push("session artifact files were truncated".to_string());
    }
    if artifacts.sources_truncated {
        stats
            .warnings
            .push("session URL sources were truncated".to_string());
    }

    for (idx, file) in artifacts.files.into_iter().take(80).enumerate() {
        let recency = (80usize.saturating_sub(idx).min(80)) as i32;
        let base = if file.kind == "modified" { 735 } else { 610 };
        let boost = matcher.boost(&[&file.path, &file.kind]);
        upsert_candidate(
            map,
            format!("file:{}", file.path),
            ContextCandidate {
                id: format!("file:{}", file.path),
                kind: ContextCandidateKind::File,
                title: display_path(&file.path),
                subtitle: Some(file.path.clone()),
                path: Some(file.path.clone()),
                line: None,
                url: None,
                score: 0,
                reasons: Vec::new(),
                sources: Vec::new(),
                status: Some(file.kind.clone()),
                metadata: json!({
                    "origin": "session_artifact",
                    "kind": file.kind,
                    "linesAdded": file.lines_added,
                    "linesRemoved": file.lines_removed,
                    "readLines": file.read_lines,
                }),
            },
            base + recency + boost,
            if file.kind == "modified" {
                "本会话最近修改过这个文件"
            } else {
                "本会话最近读取过这个文件"
            },
            "artifacts",
        );
    }

    for (idx, source) in artifacts.sources.into_iter().take(20).enumerate() {
        let boost = matcher.boost(&[&source.url, &source.origin]);
        let title = source
            .url
            .split('/')
            .filter(|s| !s.is_empty())
            .next_back()
            .unwrap_or(source.url.as_str())
            .to_string();
        upsert_candidate(
            map,
            format!("url:{}", source.url),
            ContextCandidate {
                id: format!("url:{}", source.url),
                kind: ContextCandidateKind::UrlSource,
                title,
                subtitle: Some(source.url.clone()),
                path: None,
                line: None,
                url: Some(source.url.clone()),
                score: 0,
                reasons: Vec::new(),
                sources: Vec::new(),
                status: Some(source.origin.clone()),
                metadata: json!({ "origin": source.origin }),
            },
            430 + (20usize.saturating_sub(idx).min(20) as i32) + boost,
            "本会话引用过这个来源",
            "artifacts",
        );
    }
}

async fn gather_lsp_diagnostics(
    db: &SessionDB,
    session_id: &str,
    matcher: &QueryMatcher,
    map: &mut HashMap<String, CandidateAccum>,
    stats: &mut ContextRetrievalStats,
) {
    let Ok(snapshot) = crate::lsp::diagnostics_for_session(db, session_id).await else {
        return;
    };
    stats.diagnostics = snapshot.diagnostics.len();
    for (idx, diagnostic) in snapshot.diagnostics.into_iter().take(80).enumerate() {
        let path = diagnostic
            .path
            .clone()
            .unwrap_or_else(|| diagnostic.uri.clone());
        let source = diagnostic
            .source
            .clone()
            .unwrap_or_else(|| "lsp".to_string());
        let severity_score = match diagnostic.severity.as_str() {
            "error" => 890,
            "warning" => 805,
            "information" => 690,
            "hint" => 625,
            _ => 580,
        };
        let boost = matcher.boost(&[
            &path,
            &diagnostic.message,
            &diagnostic.severity,
            source.as_str(),
        ]);
        upsert_candidate(
            map,
            format!(
                "diagnostic:{}:{}:{}",
                path, diagnostic.range.start_line, diagnostic.range.start_column
            ),
            ContextCandidate {
                id: format!(
                    "diagnostic:{}:{}:{}",
                    path, diagnostic.range.start_line, diagnostic.range.start_column
                ),
                kind: ContextCandidateKind::Diagnostic,
                title: diagnostic.message.clone(),
                subtitle: Some(path.clone()),
                path: Some(path),
                line: Some(diagnostic.range.start_line),
                url: None,
                score: 0,
                reasons: Vec::new(),
                sources: Vec::new(),
                status: Some(diagnostic.severity.clone()),
                metadata: json!({
                    "origin": "lsp_diagnostic",
                    "source": source,
                    "code": diagnostic.code,
                    "range": diagnostic.range,
                }),
            },
            severity_score - idx.min(50) as i32 + boost,
            "语言服务报告了这里的诊断",
            "lsp",
        );
    }
}

fn gather_review_findings(
    db: &SessionDB,
    session_id: &str,
    matcher: &QueryMatcher,
    map: &mut HashMap<String, CandidateAccum>,
    stats: &mut ContextRetrievalStats,
) {
    let Ok(runs) = db.list_review_runs_for_session(session_id, REVIEW_RUN_LIMIT) else {
        return;
    };
    let mut seen = HashSet::new();
    for run in runs {
        let Ok(findings) = db.list_review_findings_for_run(&run.id) else {
            continue;
        };
        for finding in findings {
            if !seen.insert(finding.id.clone()) {
                continue;
            }
            stats.review_findings += 1;
            let base = review_score(finding.severity, finding.status);
            let boost = matcher.boost(&[
                &finding.file,
                &finding.title,
                &finding.body,
                &finding.category,
                finding.severity.as_str(),
                finding.status.as_str(),
            ]);
            upsert_candidate(
                map,
                format!("review:{}", finding.id),
                ContextCandidate {
                    id: format!("review:{}", finding.id),
                    kind: ContextCandidateKind::ReviewFinding,
                    title: finding.title,
                    subtitle: Some(finding.file.clone()),
                    path: Some(finding.file),
                    line: finding.start_line,
                    url: None,
                    score: 0,
                    reasons: Vec::new(),
                    sources: Vec::new(),
                    status: Some(format!(
                        "{}:{}:{}",
                        finding.severity.as_str(),
                        finding.verdict.as_str(),
                        finding.status.as_str()
                    )),
                    metadata: json!({
                        "origin": "review_finding",
                        "runId": run.id,
                        "findingId": finding.id,
                        "severity": finding.severity,
                        "verdict": finding.verdict,
                        "status": finding.status,
                        "category": finding.category,
                        "body": finding.body,
                    }),
                },
                base + boost,
                "代码审查把这里标成待关注项",
                "review",
            );
        }
    }
}

fn gather_verification_steps(
    db: &SessionDB,
    session_id: &str,
    matcher: &QueryMatcher,
    map: &mut HashMap<String, CandidateAccum>,
    stats: &mut ContextRetrievalStats,
) {
    let Ok(runs) = db.list_verification_runs_for_session(session_id, VERIFICATION_RUN_LIMIT) else {
        return;
    };
    let mut seen = HashSet::new();
    for run in runs {
        let Ok(steps) = db.list_verification_steps_for_run(&run.id) else {
            continue;
        };
        for step in steps {
            if !seen.insert(step.id.clone()) {
                continue;
            }
            stats.verification_steps += 1;
            let base = verification_score(step.state);
            let boost = matcher.boost(&[
                &step.command,
                &step.title,
                &step.reason,
                &step.category,
                step.state.as_str(),
                step.risk.as_str(),
            ]);
            upsert_candidate(
                map,
                format!("verification:{}", step.id),
                ContextCandidate {
                    id: format!("verification:{}", step.id),
                    kind: ContextCandidateKind::VerificationStep,
                    title: step.title,
                    subtitle: Some(step.command.clone()),
                    path: None,
                    line: None,
                    url: None,
                    score: 0,
                    reasons: Vec::new(),
                    sources: Vec::new(),
                    status: Some(step.state.as_str().to_string()),
                    metadata: json!({
                        "origin": "verification_step",
                        "runId": run.id,
                        "stepId": step.id,
                        "command": step.command,
                        "cwd": step.cwd,
                        "reason": step.reason,
                        "category": step.category,
                        "risk": step.risk,
                        "autoRun": step.auto_run,
                        "exitCode": step.exit_code,
                        "outputPreview": step.output_preview,
                        "durationMs": step.duration_ms,
                    }),
                },
                base + boost,
                "验证计划或结果提示这里需要关注",
                "verification",
            );
        }
    }
}

async fn gather_file_search(
    workspace_root: Option<&str>,
    query: Option<&str>,
    matcher: &QueryMatcher,
    map: &mut HashMap<String, CandidateAccum>,
    stats: &mut ContextRetrievalStats,
) {
    let Some(root) = workspace_root.map(str::to_string) else {
        return;
    };
    let Some(query) = query.map(str::to_string) else {
        return;
    };
    let search = tokio::task::spawn_blocking(move || {
        crate::filesystem::search_files(&root, &query, Some(FILE_SEARCH_LIMIT))
    })
    .await;
    let Ok(Ok(response)) = search else {
        return;
    };
    stats.file_search_matches = response.matches.len();
    if response.truncated {
        stats
            .warnings
            .push("file search reached the walk cap".to_string());
    }
    for file in response.matches {
        let boost = matcher.boost(&[&file.path, &file.rel_path, &file.name]);
        upsert_candidate(
            map,
            format!("file:{}", file.path),
            ContextCandidate {
                id: format!("file:{}", file.path),
                kind: ContextCandidateKind::File,
                title: file.name.clone(),
                subtitle: Some(file.rel_path.clone()),
                path: Some(file.path),
                line: None,
                url: None,
                score: 0,
                reasons: Vec::new(),
                sources: Vec::new(),
                status: if file.is_dir {
                    Some("directory".to_string())
                } else {
                    Some("file".to_string())
                },
                metadata: json!({
                    "origin": "file_search",
                    "relPath": file.rel_path,
                    "isDir": file.is_dir,
                    "fileSearchScore": file.score,
                }),
            },
            510 + (file.score / 80).clamp(0, 260) + boost,
            "文件名或路径匹配当前查询",
            "file_search",
        );
    }
}

async fn gather_lsp_symbols(
    db: &SessionDB,
    session_id: &str,
    query: Option<&str>,
    matcher: &QueryMatcher,
    map: &mut HashMap<String, CandidateAccum>,
    stats: &mut ContextRetrievalStats,
) {
    let Some(query) = query else {
        return;
    };
    if query.chars().count() < 2 {
        return;
    }
    let Ok(snapshot) =
        crate::lsp::workspace_symbols_for_session(db, session_id, query, Some(SYMBOL_SEARCH_LIMIT))
            .await
    else {
        return;
    };
    stats.symbols = snapshot.symbols.len();
    for warning in snapshot.errors {
        stats.warnings.push(warning);
    }
    for (idx, symbol) in snapshot.symbols.into_iter().enumerate() {
        let path = symbol.path.clone();
        let line = symbol.range.as_ref().map(|r| r.start_line);
        let detail = symbol.detail.clone().unwrap_or_default();
        let boost = matcher.boost(&[
            &symbol.name,
            &detail,
            symbol.kind.as_deref().unwrap_or("symbol"),
            path.as_deref().unwrap_or(""),
            &symbol.server,
        ]);
        upsert_candidate(
            map,
            format!(
                "symbol:{}:{}:{}",
                symbol.name,
                path.as_deref().unwrap_or(""),
                line.unwrap_or(0)
            ),
            ContextCandidate {
                id: format!(
                    "symbol:{}:{}:{}",
                    symbol.name,
                    path.as_deref().unwrap_or(""),
                    line.unwrap_or(0)
                ),
                kind: ContextCandidateKind::Symbol,
                title: symbol.name,
                subtitle: symbol
                    .kind
                    .clone()
                    .or(symbol.detail.clone())
                    .or_else(|| path.clone()),
                path,
                line,
                url: None,
                score: 0,
                reasons: Vec::new(),
                sources: Vec::new(),
                status: symbol.kind.clone(),
                metadata: json!({
                    "origin": "lsp_symbol",
                    "server": symbol.server,
                    "kind": symbol.kind,
                    "detail": symbol.detail,
                    "range": symbol.range,
                }),
            },
            700 + boost - idx.min(50) as i32,
            "语义符号匹配当前查询",
            "lsp",
        );
    }
}

fn upsert_candidate(
    map: &mut HashMap<String, CandidateAccum>,
    key: String,
    mut candidate: ContextCandidate,
    rank: i32,
    reason: &str,
    source: &str,
) {
    candidate.score = rank.max(0) as u32;
    candidate.reasons.push(reason.to_string());
    candidate.sources.push(source.to_string());
    if let Some(existing) = map.get_mut(&key) {
        if rank > existing.rank {
            let mut reasons = existing.candidate.reasons.clone();
            let mut sources = existing.candidate.sources.clone();
            add_unique(&mut reasons, reason);
            add_unique(&mut sources, source);
            candidate.reasons = reasons;
            candidate.sources = sources;
            candidate.score = rank.max(0) as u32;
            existing.candidate = candidate;
            existing.rank = rank;
        } else {
            add_unique(&mut existing.candidate.reasons, reason);
            add_unique(&mut existing.candidate.sources, source);
        }
        return;
    }
    map.insert(key, CandidateAccum { candidate, rank });
}

fn add_unique(list: &mut Vec<String>, value: &str) {
    if !list.iter().any(|item| item == value) {
        list.push(value.to_string());
    }
}

fn review_score(severity: ReviewSeverity, status: ReviewFindingStatus) -> i32 {
    let base = match severity {
        ReviewSeverity::P0 => 985,
        ReviewSeverity::P1 => 935,
        ReviewSeverity::P2 => 855,
        ReviewSeverity::P3 => 720,
    };
    match status {
        ReviewFindingStatus::Open => base,
        ReviewFindingStatus::Resolved => base - 260,
        ReviewFindingStatus::Dismissed | ReviewFindingStatus::FalsePositive => base - 340,
    }
}

fn verification_score(state: VerificationStepState) -> i32 {
    match state {
        VerificationStepState::Failed | VerificationStepState::TimedOut => 910,
        VerificationStepState::Running => 820,
        VerificationStepState::Pending => 735,
        VerificationStepState::Skipped => 650,
        VerificationStepState::Passed => 520,
    }
}

fn kind_rank(kind: &ContextCandidateKind) -> u8 {
    match kind {
        ContextCandidateKind::ReviewFinding => 0,
        ContextCandidateKind::Diagnostic => 1,
        ContextCandidateKind::VerificationStep => 2,
        ContextCandidateKind::File => 3,
        ContextCandidateKind::Symbol => 4,
        ContextCandidateKind::UrlSource => 5,
    }
}

fn display_path(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_string()
}
