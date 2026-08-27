//! Skill step: put a `fallow` skill where each harness discovers it.
//!
//! When the project has `node_modules/fallow/skills/fallow`, a small stub
//! skill points there so the installed copy never drifts from the binary in
//! `node_modules`. Otherwise the tree embedded at build time is written.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use super::{
    Ctx, Harness, MARKER_PREFIX, MARKER_VERSION, Reason, Scope, Step, StepReport, StepStatus,
};
use crate::setup_hooks::read_optional_text;

include!(concat!(env!("OUT_DIR"), "/embedded_skill.rs"));

const SKILL_NAME: &str = "fallow";
const NODE_MODULES_SKILL: &str = "node_modules/fallow/skills/fallow";

/// How the installed skill was produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavor {
    Stub,
    Embedded,
}

impl Flavor {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Stub => "stub",
            Self::Embedded => "embedded",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "stub" => Some(Self::Stub),
            "embedded" => Some(Self::Embedded),
            _ => None,
        }
    }
}

/// State of a skill directory on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkillState {
    Absent,
    Managed { flavor: Flavor, version: String },
    Foreign,
}

/// Where the skill goes for a set of harnesses. Codex and Cursor share the
/// cross-harness `.agents/skills/` root; Claude Code reads `.claude/skills/`.
#[derive(Clone, Debug)]
pub struct Target {
    pub harness: Option<Harness>,
    pub dir: PathBuf,
    pub scope: Scope,
    pub covers: Vec<Harness>,
}

pub fn targets(ctx: &Ctx, harnesses: &[Harness]) -> Result<Vec<Target>, String> {
    let base = ctx.scope_base()?;
    let mut targets: Vec<Target> = Vec::new();
    let neutral: Vec<Harness> = harnesses
        .iter()
        .copied()
        .filter(|h| matches!(h, Harness::Codex | Harness::Cursor))
        .collect();
    if harnesses.is_empty() || !neutral.is_empty() {
        targets.push(Target {
            harness: None,
            dir: base.join(".agents").join("skills").join(SKILL_NAME),
            scope: ctx.scope(),
            covers: neutral,
        });
    }
    if harnesses.contains(&Harness::Claude) {
        targets.push(Target {
            harness: Some(Harness::Claude),
            dir: base.join(".claude").join("skills").join(SKILL_NAME),
            scope: ctx.scope(),
            covers: vec![Harness::Claude],
        });
    }
    Ok(targets)
}

pub fn install(ctx: &Ctx, harnesses: &[Harness]) -> Vec<StepReport> {
    let targets = match targets(ctx, harnesses) {
        Ok(targets) => targets,
        Err(message) => {
            return vec![StepReport::failed(None, Step::Skill, Scope::Local, message)];
        }
    };
    let source = match Source::resolve(&ctx.root) {
        Ok(source) => source,
        Err(reason) => {
            return targets
                .into_iter()
                .map(|target| {
                    StepReport::new(target.harness, Step::Skill, StepStatus::Skipped, target.scope)
                        .path(ctx, &target.dir)
                        .reason(reason)
                        .detail(
                            "install fallow through npm so the skill ships with the binary, or build fallow from a full checkout",
                        )
                })
                .collect();
        }
    };
    targets
        .into_iter()
        .map(|target| install_one(ctx, &target, &source))
        .collect()
}

pub fn uninstall(ctx: &Ctx, harnesses: &[Harness]) -> Vec<StepReport> {
    let targets = match targets(ctx, harnesses) {
        Ok(targets) => targets,
        Err(message) => {
            return vec![StepReport::failed(None, Step::Skill, Scope::Local, message)];
        }
    };
    targets
        .into_iter()
        .map(|target| uninstall_one(ctx, &target))
        .collect()
}

enum Source {
    Stub {
        frontmatter: String,
        openai_yaml: Option<String>,
    },
    Embedded,
}

impl Source {
    fn resolve(root: &Path) -> Result<Self, Reason> {
        let shipped = root.join(NODE_MODULES_SKILL).join("SKILL.md");
        if let Ok(Some(text)) = read_optional_text(&shipped)
            && let Some(frontmatter) = frontmatter_block(&text)
        {
            let openai_yaml = read_optional_text(
                &root
                    .join(NODE_MODULES_SKILL)
                    .join("agents")
                    .join("openai.yaml"),
            )
            .ok()
            .flatten();
            return Ok(Self::Stub {
                frontmatter: frontmatter.to_string(),
                openai_yaml,
            });
        }
        if EMBEDDED_SKILL.is_empty() {
            return Err(Reason::SkillNotEmbedded);
        }
        Ok(Self::Embedded)
    }

    const fn flavor(&self) -> Flavor {
        match self {
            Self::Stub { .. } => Flavor::Stub,
            Self::Embedded => Flavor::Embedded,
        }
    }

    /// Files to write, relative to the skill directory.
    fn files(&self) -> Result<Vec<(String, Vec<u8>)>, String> {
        match self {
            Self::Stub {
                frontmatter,
                openai_yaml,
            } => {
                let mut files =
                    vec![("SKILL.md".to_string(), stub_skill(frontmatter).into_bytes())];
                if let Some(yaml) = openai_yaml {
                    files.push(("agents/openai.yaml".to_string(), yaml.clone().into_bytes()));
                }
                Ok(files)
            }
            Self::Embedded => EMBEDDED_SKILL
                .iter()
                .map(|file| {
                    let mut decoder = flate2::read::GzDecoder::new(file.gzip);
                    let mut bytes = Vec::with_capacity(file.raw_len);
                    decoder.read_to_end(&mut bytes).map_err(|e| {
                        format!("decompress embedded skill file {}: {e}", file.path)
                    })?;
                    if file.path == "SKILL.md" {
                        let text = String::from_utf8(bytes)
                            .map_err(|e| format!("embedded SKILL.md is not UTF-8: {e}"))?;
                        bytes = with_marker(&text, Flavor::Embedded).into_bytes();
                    }
                    Ok((file.path.to_string(), bytes))
                })
                .collect(),
        }
    }
}

fn marker_line(flavor: Flavor) -> String {
    format!(
        "<!-- {MARKER_PREFIX} {MARKER_VERSION} skill={} version={} -->",
        flavor.as_str(),
        env!("CARGO_PKG_VERSION")
    )
}

/// The YAML frontmatter block including both `---` fences and the trailing
/// newline, or `None` when the text does not start with one.
fn frontmatter_block(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("---\n")?;
    let close = rest.find("\n---\n")?;
    Some(&text[..4 + close + 5])
}

/// Insert the marker line right after the frontmatter (or at the top when
/// there is none) so the frontmatter parser of every harness stays happy.
fn with_marker(text: &str, flavor: Flavor) -> String {
    let marker = marker_line(flavor);
    match frontmatter_block(text) {
        Some(front) => format!("{front}{marker}\n{}", &text[front.len()..]),
        None => format!("{marker}\n{text}"),
    }
}

fn stub_skill(frontmatter: &str) -> String {
    format!(
        "{frontmatter}{}\n\n# Fallow\n\n\
This pointer skill was written by `fallow agent install`. The complete, version-matched skill ships inside the installed npm package:\n\n\
- `{NODE_MODULES_SKILL}/SKILL.md` (start here)\n\
- `{NODE_MODULES_SKILL}/references/` (CLI reference, MCP tools, patterns, gotchas)\n\n\
Read that `SKILL.md` before running fallow. Resolve current flags from `fallow --help` and `fallow <command> --help`, never from memory.\n\n\
## Fallow task map\n\n{}",
        marker_line(Flavor::Stub),
        crate::task_matrix::render_task_matrix_markdown()
    )
}

/// Inspect a skill directory without touching it.
pub fn inspect(dir: &Path) -> SkillState {
    let Ok(Some(text)) = read_optional_text(&dir.join("SKILL.md")) else {
        return SkillState::Absent;
    };
    parse_marker(&text).map_or(SkillState::Foreign, |(flavor, version)| {
        SkillState::Managed { flavor, version }
    })
}

fn parse_marker(text: &str) -> Option<(Flavor, String)> {
    let prefix = format!("<!-- {MARKER_PREFIX} {MARKER_VERSION} skill=");
    let line = text
        .lines()
        .take(40)
        .find(|line| line.starts_with(&prefix))?;
    let rest = line.strip_prefix(&prefix)?;
    let (flavor, rest) = rest.split_once(' ')?;
    let version = rest.strip_prefix("version=")?.strip_suffix(" -->")?;
    Some((Flavor::parse(flavor)?, version.to_string()))
}

fn install_one(ctx: &Ctx, target: &Target, source: &Source) -> StepReport {
    let base = StepReport::new(
        target.harness,
        Step::Skill,
        StepStatus::Written,
        target.scope,
    )
    .path(ctx, &target.dir);
    let base = match target.harness {
        Some(_) => base,
        None if target.covers.is_empty() => base,
        None => base.detail(format!(
            "read by {}",
            target
                .covers
                .iter()
                .map(|h| h.display_name())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    };

    match inspect(&target.dir) {
        SkillState::Foreign if !ctx.force => {
            return base
                .with_status(StepStatus::Refused)
                .reason(Reason::SkillNameTaken)
                .detail("a skill named `fallow` already exists here without a fallow marker; pass --force to replace it");
        }
        SkillState::Absent | SkillState::Managed { .. } | SkillState::Foreign => {}
    }

    let files = match source.files() {
        Ok(files) => files,
        Err(message) => {
            return StepReport::failed(target.harness, Step::Skill, target.scope, message)
                .path(ctx, &target.dir);
        }
    };

    let mut changed = false;
    for (relative, bytes) in &files {
        let path = target.dir.join(relative);
        let current = std::fs::read(&path).ok();
        if current.as_deref() == Some(bytes.as_slice()) {
            continue;
        }
        changed = true;
        if ctx.dry_run {
            continue;
        }
        if let Some(parent) = path.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            return StepReport::failed(
                target.harness,
                Step::Skill,
                target.scope,
                error.to_string(),
            )
            .path(ctx, &path);
        }
        if let Err(error) = std::fs::write(&path, bytes) {
            return StepReport::failed(
                target.harness,
                Step::Skill,
                target.scope,
                error.to_string(),
            )
            .path(ctx, &path);
        }
    }

    let flavor = source.flavor();
    let detail = match flavor {
        Flavor::Stub => format!("pointer to {NODE_MODULES_SKILL}"),
        Flavor::Embedded => format!("embedded copy, {} files", files.len()),
    };
    let mut report = if changed {
        base
    } else {
        base.with_status(StepStatus::Unchanged)
    };
    report.detail = Some(match report.detail.take() {
        Some(existing) => format!("{detail}; {existing}"),
        None => detail,
    });
    report
}

/// Every relative path a managed skill directory may contain.
fn managed_files() -> Vec<String> {
    let mut files: Vec<String> = EMBEDDED_SKILL.iter().map(|f| f.path.to_string()).collect();
    for known in ["SKILL.md", "agents/openai.yaml"] {
        if !files.iter().any(|f| f == known) {
            files.push(known.to_string());
        }
    }
    files
}

fn uninstall_one(ctx: &Ctx, target: &Target) -> StepReport {
    let base = StepReport::new(
        target.harness,
        Step::Skill,
        StepStatus::Removed,
        target.scope,
    )
    .path(ctx, &target.dir);
    match inspect(&target.dir) {
        SkillState::Absent => {
            return base
                .with_status(StepStatus::Unchanged)
                .detail("not present");
        }
        SkillState::Foreign if !ctx.force => {
            return base
                .with_status(StepStatus::Skipped)
                .reason(Reason::SkillNameTaken)
                .detail("not written by fallow; left untouched (pass --force to remove)");
        }
        SkillState::Managed { .. } | SkillState::Foreign => {}
    }
    if ctx.dry_run {
        return base;
    }
    for relative in managed_files() {
        let path = target.dir.join(&relative);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return StepReport::failed(
                    target.harness,
                    Step::Skill,
                    target.scope,
                    error.to_string(),
                )
                .path(ctx, &path);
            }
        }
    }
    for sub in ["references", "agents"] {
        let _ = std::fs::remove_dir(target.dir.join(sub));
    }
    let _ = std::fs::remove_dir(&target.dir);
    if let Some(parent) = target.dir.parent() {
        let _ = std::fs::remove_dir(parent);
    }
    base
}
