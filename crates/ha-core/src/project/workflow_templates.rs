//! Project-local Agent Workflow Spec discovery.
//!
//! This is a read-only adapter over `<project.working_dir>/.agent-workflows/`.
//! It intentionally does not create a workflow engine, persist templates, or call
//! any execution APIs. The output is a compact preview shape that the owner UI can
//! display before a future orchestration handoff.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use super::db::ProjectDB;

const WORKFLOW_DIR: &str = ".agent-workflows";
const EXPECTED_ROOT_FILES: &[&str] = &[
    "project.yaml",
    "artifact-schema.yaml",
    "progress.schema.json",
    "gates.yaml",
    "verification.yaml",
    "README.md",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWorkflowDiscovery {
    pub project_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
    pub workflows_dir: String,
    pub exists: bool,
    #[serde(default)]
    pub missing_files: Vec<String>,
    #[serde(default)]
    pub templates: Vec<ProjectWorkflowTemplateSummary>,
    #[serde(default)]
    pub modes: Vec<ProjectWorkflowModeSummary>,
    #[serde(default)]
    pub fixed_artifacts: Vec<ProjectWorkflowFixedArtifact>,
    #[serde(default)]
    pub verification_commands: Vec<ProjectWorkflowVerificationCommand>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWorkflowTemplateSummary {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub task_types: Vec<String>,
    pub phase_count: usize,
    #[serde(default)]
    pub modes: Vec<String>,
    pub fixed_artifacts_count: usize,
    #[serde(default)]
    pub source_files: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWorkflowModeSummary {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub source_files: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWorkflowFixedArtifact {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default)]
    pub source_files: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWorkflowVerificationCommand {
    pub id: String,
    pub command: String,
    #[serde(default)]
    pub source_files: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewProjectWorkflowInput {
    pub project_id: String,
    pub template_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWorkflowPreview {
    pub project_id: String,
    pub template_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_type: Option<String>,
    #[serde(default)]
    pub phases: Vec<ProjectWorkflowPhasePreview>,
    #[serde(default)]
    pub fixed_artifacts: Vec<ProjectWorkflowFixedArtifact>,
    #[serde(default)]
    pub required_interactions: Vec<ProjectWorkflowRequiredInteraction>,
    #[serde(default)]
    pub verification_commands: Vec<ProjectWorkflowVerificationCommand>,
    #[serde(default)]
    pub source_files: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWorkflowPhasePreview {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub required_interactions: Vec<ProjectWorkflowRequiredInteraction>,
    #[serde(default)]
    pub fixed_artifacts: Vec<ProjectWorkflowFixedArtifact>,
    #[serde(default)]
    pub verification_commands: Vec<ProjectWorkflowVerificationCommand>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWorkflowRequiredInteraction {
    pub kind: String,
    pub prompt: String,
}

#[derive(Debug, Clone, Default)]
struct ParsedWorkflowFile {
    templates: Vec<ParsedTemplate>,
    modes: Vec<ProjectWorkflowModeSummary>,
    fixed_artifacts: Vec<ProjectWorkflowFixedArtifact>,
    verification_commands: Vec<ProjectWorkflowVerificationCommand>,
}

#[derive(Debug, Clone, Default)]
struct ParsedTemplate {
    summary: ProjectWorkflowTemplateSummary,
    phases: Vec<ProjectWorkflowPhasePreview>,
    fixed_artifacts: Vec<ProjectWorkflowFixedArtifact>,
    verification_commands: Vec<ProjectWorkflowVerificationCommand>,
    required_interactions: Vec<ProjectWorkflowRequiredInteraction>,
}

pub fn discover_project_workflows(
    project_id: &str,
    project_db: &ProjectDB,
) -> Result<ProjectWorkflowDiscovery> {
    let project = project_db
        .get(project_id)?
        .ok_or_else(|| anyhow::anyhow!("project not found: {}", project_id))?;
    let Some(working_dir) = project.working_dir.clone() else {
        return Ok(ProjectWorkflowDiscovery {
            project_id: project_id.to_string(),
            working_dir: None,
            workflows_dir: WORKFLOW_DIR.to_string(),
            exists: false,
            missing_files: expected_workflow_files(),
            templates: Vec::new(),
            modes: Vec::new(),
            fixed_artifacts: Vec::new(),
            verification_commands: Vec::new(),
        });
    };

    let root = PathBuf::from(&working_dir).join(WORKFLOW_DIR);
    let workflows_dir = root.to_string_lossy().to_string();
    if !root.is_dir() {
        return Ok(ProjectWorkflowDiscovery {
            project_id: project_id.to_string(),
            working_dir: Some(working_dir),
            workflows_dir,
            exists: false,
            missing_files: expected_workflow_files(),
            templates: Vec::new(),
            modes: Vec::new(),
            fixed_artifacts: Vec::new(),
            verification_commands: Vec::new(),
        });
    }

    let missing_files = expected_workflow_files()
        .into_iter()
        .filter(|name| !root.join(name).is_file())
        .collect::<Vec<_>>();
    let parsed = parse_workflow_dir(&root)?;

    Ok(ProjectWorkflowDiscovery {
        project_id: project_id.to_string(),
        working_dir: Some(working_dir),
        workflows_dir,
        exists: true,
        missing_files,
        templates: parsed.templates.into_iter().map(|t| t.summary).collect(),
        modes: parsed.modes,
        fixed_artifacts: parsed.fixed_artifacts,
        verification_commands: parsed.verification_commands,
    })
}

pub fn preview_project_workflow(
    input: PreviewProjectWorkflowInput,
    project_db: &ProjectDB,
) -> Result<ProjectWorkflowPreview> {
    let project = project_db
        .get(&input.project_id)?
        .ok_or_else(|| anyhow::anyhow!("project not found: {}", input.project_id))?;
    let working_dir = project
        .working_dir
        .ok_or_else(|| anyhow::anyhow!("project has no working_dir: {}", input.project_id))?;
    let root = PathBuf::from(working_dir).join(WORKFLOW_DIR);
    if !root.is_dir() {
        bail!("project workflow directory not found: {}", root.display());
    }

    let parsed = parse_workflow_dir(&root)?;
    let template = parsed
        .templates
        .into_iter()
        .find(|template| template.summary.id == input.template_id)
        .ok_or_else(|| {
            anyhow::anyhow!("project workflow template not found: {}", input.template_id)
        })?;

    if let Some(task_type) = input.task_type.as_deref() {
        if !template.summary.task_types.is_empty()
            && !template
                .summary
                .task_types
                .iter()
                .any(|item| item == task_type)
        {
            bail!(
                "taskType '{}' is not supported by template '{}'",
                task_type,
                input.template_id
            );
        }
    }
    if let Some(mode) = input.mode.as_deref() {
        let known_mode = parsed.modes.iter().any(|item| item.id == mode);
        let template_mode = template.summary.modes.iter().any(|item| item == mode);
        if !known_mode || (!template.summary.modes.is_empty() && !template_mode) {
            bail!(
                "mode '{}' is not supported by template '{}'",
                mode,
                input.template_id
            );
        }
    }

    let mut source_files = BTreeSet::new();
    for file in &template.summary.source_files {
        source_files.insert(file.clone());
    }
    if let Some(mode) = input.mode.as_deref() {
        if let Some(mode_summary) = parsed.modes.iter().find(|item| item.id == mode) {
            for file in &mode_summary.source_files {
                source_files.insert(file.clone());
            }
        }
    }
    for phase in &template.phases {
        for artifact in &phase.fixed_artifacts {
            for file in &artifact.source_files {
                source_files.insert(file.clone());
            }
        }
        for command in &phase.verification_commands {
            for file in &command.source_files {
                source_files.insert(file.clone());
            }
        }
    }
    for artifact in &template.fixed_artifacts {
        for file in &artifact.source_files {
            source_files.insert(file.clone());
        }
    }
    for command in &template.verification_commands {
        for file in &command.source_files {
            source_files.insert(file.clone());
        }
    }

    let fixed_artifacts = template
        .fixed_artifacts
        .iter()
        .cloned()
        .chain(
            template
                .phases
                .iter()
                .flat_map(|phase| phase.fixed_artifacts.clone()),
        )
        .collect::<Vec<_>>();
    let verification_commands = template
        .verification_commands
        .iter()
        .cloned()
        .chain(
            template
                .phases
                .iter()
                .flat_map(|phase| phase.verification_commands.clone()),
        )
        .collect::<Vec<_>>();
    let required_interactions = template
        .phases
        .iter()
        .flat_map(|phase| phase.required_interactions.clone())
        .chain(template.required_interactions.clone())
        .collect::<Vec<_>>();

    Ok(ProjectWorkflowPreview {
        project_id: input.project_id,
        template_id: input.template_id,
        mode: input.mode,
        task_type: input.task_type,
        phases: template.phases,
        fixed_artifacts,
        required_interactions,
        verification_commands,
        source_files: source_files.into_iter().collect(),
    })
}

fn parse_workflow_dir(root: &Path) -> Result<ParsedWorkflowFile> {
    let mut merged = ParsedWorkflowFile::default();
    for file in workflow_source_files(root)? {
        let source_file = file
            .strip_prefix(root)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        let content = fs::read_to_string(&file)?;
        let parsed = parse_workflow_text(&content, &source_file);
        merged.templates.extend(parsed.templates);
        merged.modes.extend(parsed.modes);
        merged.fixed_artifacts.extend(parsed.fixed_artifacts);
        merged
            .verification_commands
            .extend(parsed.verification_commands);
    }

    let global_modes = merged
        .modes
        .iter()
        .map(|mode| mode.id.clone())
        .collect::<Vec<_>>();
    for template in &mut merged.templates {
        if template.summary.modes.is_empty() {
            template.summary.modes = global_modes.clone();
        }
        if template.fixed_artifacts.is_empty() {
            template.fixed_artifacts = merged.fixed_artifacts.clone();
        }
        if template.verification_commands.is_empty() {
            template.verification_commands = merged.verification_commands.clone();
        }
        if template.summary.fixed_artifacts_count == 0 {
            let phase_artifact_count = template
                .phases
                .iter()
                .map(|phase| phase.fixed_artifacts.len())
                .sum::<usize>();
            template.summary.fixed_artifacts_count =
                template.fixed_artifacts.len().max(phase_artifact_count);
        }
        if template.summary.phase_count == 0 {
            template.summary.phase_count = template.phases.len();
        }
    }
    Ok(merged)
}

fn workflow_source_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for expected in EXPECTED_ROOT_FILES {
        let file = root.join(expected);
        if file.is_file() {
            files.push(file);
        }
    }
    for dir_name in ["templates", "modes"] {
        let dir = root.join(dir_name);
        if !dir.is_dir() {
            continue;
        }
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
                continue;
            };
            if matches!(ext, "yaml" | "yml") && path.is_file() {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

fn parse_workflow_text(content: &str, source_file: &str) -> ParsedWorkflowFile {
    let lines = content.lines().collect::<Vec<_>>();
    if source_file.starts_with("templates/") {
        return ParsedWorkflowFile {
            templates: parse_template_file(&lines, source_file),
            modes: Vec::new(),
            fixed_artifacts: Vec::new(),
            verification_commands: Vec::new(),
        };
    }
    if source_file.starts_with("modes/") {
        return ParsedWorkflowFile {
            templates: Vec::new(),
            modes: parse_mode_file(&lines, source_file),
            fixed_artifacts: Vec::new(),
            verification_commands: Vec::new(),
        };
    }
    if source_file == "project.yaml" {
        return ParsedWorkflowFile {
            templates: Vec::new(),
            modes: Vec::new(),
            fixed_artifacts: parse_fixed_artifacts(&lines, source_file),
            verification_commands: Vec::new(),
        };
    }
    if source_file == "verification.yaml" {
        return ParsedWorkflowFile {
            templates: Vec::new(),
            modes: Vec::new(),
            fixed_artifacts: Vec::new(),
            verification_commands: parse_verification_commands(&lines, source_file),
        };
    }
    ParsedWorkflowFile {
        templates: parse_templates(&lines, source_file),
        modes: parse_named_items(&lines, "modes", source_file),
        fixed_artifacts: parse_fixed_artifacts(&lines, source_file),
        verification_commands: parse_verification_commands(&lines, source_file),
    }
}

fn expected_workflow_files() -> Vec<String> {
    EXPECTED_ROOT_FILES
        .iter()
        .map(|name| (*name).to_string())
        .chain(
            [
                "modes/template-only.yaml",
                "modes/guided.yaml",
                "modes/dynamic.yaml",
            ]
            .into_iter()
            .map(|name| name.to_string()),
        )
        .chain(
            ["templates/requirement.yaml", "templates/bugfix.yaml"]
                .into_iter()
                .map(|name| name.to_string()),
        )
        .collect()
}

fn parse_template_file(lines: &[&str], source_file: &str) -> Vec<ParsedTemplate> {
    let mut templates = parse_templates(lines, source_file);
    if !templates.is_empty() {
        return templates;
    }
    let id = scalar_in_item(lines, "id").unwrap_or_else(|| {
        Path::new(source_file)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("template")
            .to_string()
    });
    let name = scalar_in_item(lines, "name")
        .or_else(|| scalar_in_item(lines, "title"))
        .unwrap_or_else(|| id.clone());
    let task_types = array_in_item(lines, "task_types")
        .or_else(|| array_in_item(lines, "taskTypes"))
        .unwrap_or_default();
    let modes = array_in_item(lines, "execution_modes")
        .or_else(|| array_in_item(lines, "executionModes"))
        .or_else(|| array_in_item(lines, "modes"))
        .unwrap_or_default();
    let phases = parse_phases(lines, source_file);
    let fixed_artifacts = parse_fixed_artifacts(lines, source_file);
    let verification_commands = parse_verification_commands(lines, source_file);
    let required_interactions = parse_interactions(lines, source_file);
    templates.push(ParsedTemplate {
        summary: ProjectWorkflowTemplateSummary {
            id,
            name,
            task_types,
            phase_count: phases.len(),
            modes,
            fixed_artifacts_count: fixed_artifacts
                .len()
                .max(phases.iter().map(|phase| phase.fixed_artifacts.len()).sum()),
            source_files: vec![source_file.to_string()],
        },
        phases,
        fixed_artifacts,
        verification_commands,
        required_interactions,
    });
    templates
}

fn parse_mode_file(lines: &[&str], source_file: &str) -> Vec<ProjectWorkflowModeSummary> {
    let id = scalar_in_item(lines, "id").unwrap_or_else(|| {
        Path::new(source_file)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("mode")
            .to_string()
    });
    let name = scalar_in_item(lines, "name")
        .or_else(|| scalar_in_item(lines, "title"))
        .unwrap_or_else(|| id.clone());
    vec![ProjectWorkflowModeSummary {
        id,
        name,
        source_files: vec![source_file.to_string()],
    }]
}

fn parse_templates(lines: &[&str], source_file: &str) -> Vec<ParsedTemplate> {
    let mut templates = Vec::new();
    for (start, end) in list_item_ranges(lines, "templates") {
        let item = &lines[start..end];
        let id =
            scalar_in_item(item, "id").unwrap_or_else(|| fallback_id("template", templates.len()));
        let name = scalar_in_item(item, "name")
            .or_else(|| scalar_in_item(item, "title"))
            .unwrap_or_else(|| id.clone());
        let task_types = array_in_item(item, "task_types")
            .or_else(|| array_in_item(item, "taskTypes"))
            .unwrap_or_default();
        let modes = array_in_item(item, "execution_modes")
            .or_else(|| array_in_item(item, "executionModes"))
            .or_else(|| array_in_item(item, "modes"))
            .unwrap_or_default();
        let phases = parse_phases(item, source_file);
        let fixed_artifacts = parse_fixed_artifacts(item, source_file);
        let verification_commands = parse_verification_commands(item, source_file);
        let fixed_artifacts_count = fixed_artifacts
            .len()
            .max(phases.iter().map(|phase| phase.fixed_artifacts.len()).sum());
        let required_interactions = parse_interactions(item, source_file);
        templates.push(ParsedTemplate {
            summary: ProjectWorkflowTemplateSummary {
                id,
                name,
                task_types,
                phase_count: phases.len(),
                modes,
                fixed_artifacts_count,
                source_files: vec![source_file.to_string()],
            },
            phases,
            fixed_artifacts,
            verification_commands,
            required_interactions,
        });
    }
    templates
}

fn parse_phases(lines: &[&str], source_file: &str) -> Vec<ProjectWorkflowPhasePreview> {
    let mut phases = Vec::new();
    for (start, end) in list_item_ranges(lines, "phases") {
        let item = &lines[start..end];
        let id = scalar_in_item(item, "id").unwrap_or_else(|| fallback_id("phase", phases.len()));
        let name = scalar_in_item(item, "name")
            .or_else(|| scalar_in_item(item, "title"))
            .unwrap_or_else(|| id.clone());
        phases.push(ProjectWorkflowPhasePreview {
            id,
            name,
            required_interactions: parse_interactions(item, source_file),
            fixed_artifacts: parse_fixed_artifacts(item, source_file),
            verification_commands: parse_verification_commands(item, source_file),
        });
    }
    phases
}

fn parse_named_items(
    lines: &[&str],
    key: &str,
    source_file: &str,
) -> Vec<ProjectWorkflowModeSummary> {
    let mut items = Vec::new();
    for (start, end) in list_item_ranges(lines, key) {
        let item = &lines[start..end];
        let id = scalar_in_item(item, "id").unwrap_or_else(|| fallback_id(key, items.len()));
        let name = scalar_in_item(item, "name")
            .or_else(|| scalar_in_item(item, "title"))
            .unwrap_or_else(|| id.clone());
        items.push(ProjectWorkflowModeSummary {
            id,
            name,
            source_files: vec![source_file.to_string()],
        });
    }
    items
}

fn parse_fixed_artifacts(lines: &[&str], source_file: &str) -> Vec<ProjectWorkflowFixedArtifact> {
    let mut items: Vec<ProjectWorkflowFixedArtifact> = Vec::new();
    for key in ["fixed_artifacts", "fixedArtifacts"] {
        if let Some(values) = array_in_item(lines, key) {
            for value in values {
                if items.iter().any(|item| item.id.as_str() == value) {
                    continue;
                }
                items.push(ProjectWorkflowFixedArtifact {
                    id: value.clone(),
                    name: value,
                    path: None,
                    source_files: vec![source_file.to_string()],
                });
            }
        }
        for (start, end) in list_item_ranges(lines, key) {
            let item = &lines[start..end];
            let id =
                scalar_in_item(item, "id").unwrap_or_else(|| fallback_id("artifact", items.len()));
            if items.iter().any(|item| item.id == id) {
                continue;
            }
            let name = scalar_in_item(item, "name")
                .or_else(|| scalar_in_item(item, "title"))
                .unwrap_or_else(|| id.clone());
            let path = scalar_in_item(item, "path");
            items.push(ProjectWorkflowFixedArtifact {
                id,
                name,
                path,
                source_files: vec![source_file.to_string()],
            });
        }
    }
    items
}

fn parse_verification_commands(
    lines: &[&str],
    source_file: &str,
) -> Vec<ProjectWorkflowVerificationCommand> {
    let mut commands = Vec::new();
    for key in [
        "verification_commands",
        "verificationCommands",
        "commands",
        "verification",
    ] {
        for (start, end) in list_item_ranges(lines, key) {
            let item = &lines[start..end];
            let command = scalar_in_item(item, "command")
                .or_else(|| scalar_in_item(item, "cmd"))
                .or_else(|| scalar_in_item(item, "alternative"))
                .or_else(|| first_dash_scalar(item));
            if let Some(command) = command {
                let id = scalar_in_item(item, "id")
                    .unwrap_or_else(|| fallback_id("verify", commands.len()));
                commands.push(ProjectWorkflowVerificationCommand {
                    id,
                    command,
                    source_files: vec![source_file.to_string()],
                });
            }
        }
    }
    for key in ["command", "alternative"] {
        if let Some(command) = scalar_in_item(lines, key) {
            commands.push(ProjectWorkflowVerificationCommand {
                id: key.replace('_', "-"),
                command,
                source_files: vec![source_file.to_string()],
            });
        }
    }
    commands
}

fn parse_interactions(
    lines: &[&str],
    source_file: &str,
) -> Vec<ProjectWorkflowRequiredInteraction> {
    let mut interactions = Vec::new();
    for key in [
        "required_interactions",
        "requiredInteractions",
        "interactions",
        "ask_user",
    ] {
        for (start, end) in list_item_ranges(lines, key) {
            let item = &lines[start..end];
            let kind = scalar_in_item(item, "kind")
                .or_else(|| scalar_in_item(item, "type"))
                .unwrap_or_else(|| "ask_user".to_string());
            let prompt = scalar_in_item(item, "prompt")
                .or_else(|| scalar_in_item(item, "question"))
                .or_else(|| first_dash_scalar(item))
                .unwrap_or_else(|| format!("required interaction from {}", source_file));
            interactions.push(ProjectWorkflowRequiredInteraction { kind, prompt });
        }
    }
    for (key, kind, prompt) in [
        (
            "discussion_required",
            "discussion",
            "Discussion is required before execution.",
        ),
        (
            "discussionRequired",
            "discussion",
            "Discussion is required before execution.",
        ),
        (
            "design_required",
            "design",
            "Design is required before development.",
        ),
        (
            "designRequired",
            "design",
            "Design is required before development.",
        ),
    ] {
        if bool_in_item(lines, key).unwrap_or(false) {
            interactions.push(ProjectWorkflowRequiredInteraction {
                kind: kind.to_string(),
                prompt: prompt.to_string(),
            });
        }
    }
    interactions
}

fn list_item_ranges(lines: &[&str], key: &str) -> Vec<(usize, usize)> {
    let Some(section_start) = lines
        .iter()
        .position(|line| normalized_key(line).as_deref() == Some(key))
    else {
        return Vec::new();
    };
    let section_indent = indent_width(lines[section_start]);
    let mut ranges = Vec::new();
    let mut current_start: Option<usize> = None;
    for idx in (section_start + 1)..lines.len() {
        let line = lines[idx];
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let indent = indent_width(line);
        if indent <= section_indent {
            break;
        }
        if line.trim_start().starts_with("- ")
            && current_start
                .map(|start| indent <= indent_width(lines[start]))
                .unwrap_or(true)
        {
            if let Some(start) = current_start.replace(idx) {
                ranges.push((start, idx));
            }
        }
    }
    if let Some(start) = current_start {
        let end = ((start + 1)..lines.len())
            .find(|idx| {
                let line = lines[*idx];
                !line.trim().is_empty()
                    && !line.trim_start().starts_with('#')
                    && indent_width(line) <= section_indent
            })
            .unwrap_or(lines.len());
        ranges.push((start, end));
    }
    ranges
}

fn scalar_in_item(lines: &[&str], key: &str) -> Option<String> {
    for line in lines {
        let trimmed = line
            .trim_start()
            .strip_prefix("- ")
            .unwrap_or(line.trim_start());
        let Some((raw_key, raw_value)) = trimmed.split_once(':') else {
            continue;
        };
        if raw_key.trim() == key {
            let value = clean_scalar(raw_value.trim());
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

fn array_in_item(lines: &[&str], key: &str) -> Option<Vec<String>> {
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line
            .trim_start()
            .strip_prefix("- ")
            .unwrap_or(line.trim_start());
        let Some((raw_key, raw_value)) = trimmed.split_once(':') else {
            continue;
        };
        if raw_key.trim() != key {
            continue;
        }
        let value = raw_value.trim();
        if value.starts_with('[') && value.ends_with(']') {
            return Some(
                value
                    .trim_matches(['[', ']'])
                    .split(',')
                    .map(clean_scalar)
                    .filter(|item| !item.is_empty())
                    .collect(),
            );
        }
        let base_indent = indent_width(line);
        let mut items = Vec::new();
        for next in &lines[(idx + 1)..] {
            if next.trim().is_empty() || next.trim_start().starts_with('#') {
                continue;
            }
            if indent_width(next) <= base_indent {
                break;
            }
            if let Some(value) = next.trim_start().strip_prefix("- ") {
                let value = clean_scalar(
                    value
                        .split_once(':')
                        .map(|(_, v)| v)
                        .unwrap_or(value)
                        .trim(),
                );
                if !value.is_empty() {
                    items.push(value);
                }
            }
        }
        return Some(items);
    }
    None
}

fn bool_in_item(lines: &[&str], key: &str) -> Option<bool> {
    scalar_in_item(lines, key).and_then(|value| match value.to_ascii_lowercase().as_str() {
        "true" | "yes" | "required" => Some(true),
        "false" | "no" | "none" => Some(false),
        _ => None,
    })
}

fn first_dash_scalar(lines: &[&str]) -> Option<String> {
    lines.iter().find_map(|line| {
        line.trim_start()
            .strip_prefix("- ")
            .map(clean_scalar)
            .filter(|value| !value.contains(':') && !value.is_empty())
    })
}

fn normalized_key(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.starts_with('-') || trimmed.starts_with('#') {
        return None;
    }
    let (key, value) = trimmed.split_once(':')?;
    if !value.trim().is_empty() {
        return None;
    }
    Some(key.trim().to_string())
}

fn indent_width(line: &str) -> usize {
    line.chars().take_while(|ch| ch.is_whitespace()).count()
}

fn clean_scalar(value: impl AsRef<str>) -> String {
    value
        .as_ref()
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string()
}

fn fallback_id(prefix: &str, idx: usize) -> String {
    format!("{}-{}", prefix.trim_end_matches('s'), idx + 1)
}
