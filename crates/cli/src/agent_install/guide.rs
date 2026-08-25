//! Guide step: `AGENTS.md` (every harness) and the `CLAUDE.md` import
//! (Claude Code only, which does not read `AGENTS.md` on its own).
//!
//! Files fallow creates whole carry an `authored` marker with a hash of the
//! content minus managed blocks. Uninstall deletes such a file only while the
//! content still hashes to that value; otherwise only managed blocks go.

use std::path::Path;

use sha2::{Digest, Sha256};

use super::{Ctx, Harness, MARKER_PREFIX, MARKER_VERSION, Scope, Step, StepReport, StepStatus};
use crate::setup_hooks::{
    AGENTS_BLOCK_START, AgentsOutcome, read_optional_text, remove_managed_block,
    strip_managed_block, upsert_managed_block,
};

const AGENTS_FILE: &str = "AGENTS.md";
const CLAUDE_FILE: &str = "CLAUDE.md";
const IMPORT_LINE: &str = "@AGENTS.md";

fn authored_marker_prefix() -> String {
    format!("<!-- {MARKER_PREFIX} {MARKER_VERSION} authored sha256=")
}

fn claude_import_start() -> String {
    format!("<!-- {MARKER_PREFIX} {MARKER_VERSION} claude-import:start -->")
}

fn claude_import_end() -> String {
    format!("<!-- {MARKER_PREFIX} {MARKER_VERSION} claude-import:end -->")
}

pub fn install(ctx: &Ctx, harnesses: &[Harness]) -> Vec<StepReport> {
    let mut steps: Vec<StepReport> = Vec::new();
    if ctx.user {
        steps.push(
            StepReport::new(None, Step::Guide, StepStatus::Skipped, Scope::Local)
                .reason("user_scope_unsupported")
                .detail("AGENTS.md and CLAUDE.md are project files; rerun without --user"),
        );
        return steps;
    }
    steps.push(install_agents_guide(ctx));
    if harnesses.contains(&Harness::Claude) {
        steps.push(install_claude_import(ctx));
    }
    steps
}

pub fn uninstall(ctx: &Ctx, harnesses: &[Harness]) -> Vec<StepReport> {
    let mut steps: Vec<StepReport> = Vec::new();
    if ctx.user {
        return steps;
    }
    if harnesses.contains(&Harness::Claude) {
        steps.push(uninstall_claude_import(ctx));
    }
    steps.push(uninstall_agents_guide(ctx));
    steps
}

fn install_agents_guide(ctx: &Ctx) -> StepReport {
    let path = ctx.root.join(AGENTS_FILE);
    let base =
        StepReport::new(None, Step::Guide, StepStatus::Written, Scope::Shared).path(ctx, &path);
    let existing = match read_optional_text(&path) {
        Ok(existing) => existing,
        Err(error) => return failed(ctx, &path, error),
    };

    if existing.is_none() {
        if ctx.dry_run {
            return base.detail("scaffold with the fallow task map");
        }
        let info = crate::init::detect_project(&ctx.root);
        let guide = crate::init::build_agents_guide(&info);
        if let Err(error) = std::fs::write(&path, guide) {
            return failed(ctx, &path, error);
        }
        if let Err(error) = upsert_managed_block(&path, false) {
            return failed(ctx, &path, error);
        }
        return match stamp_authored(&path) {
            Ok(()) => base.detail("scaffolded with the fallow task map"),
            Err(error) => failed(ctx, &path, error),
        };
    }

    match upsert_managed_block(&path, ctx.dry_run) {
        Ok(AgentsOutcome::Inserted | AgentsOutcome::Replaced) => {
            base.detail("fallow task map block")
        }
        Ok(AgentsOutcome::Unchanged) => base.with_status(StepStatus::Unchanged),
        Ok(AgentsOutcome::MalformedPreserved) => base
            .with_status(StepStatus::Refused)
            .reason("managed_block_malformed")
            .detail("fallow markers are out of order; repair AGENTS.md by hand"),
        Ok(AgentsOutcome::Removed | AgentsOutcome::NotPresent) => {
            base.with_status(StepStatus::Unchanged)
        }
        Err(error) => failed(ctx, &path, error),
    }
}

fn install_claude_import(ctx: &Ctx) -> StepReport {
    let path = ctx.root.join(CLAUDE_FILE);
    let base = StepReport::new(
        Some(Harness::Claude),
        Step::Guide,
        StepStatus::Written,
        Scope::Shared,
    )
    .path(ctx, &path);
    let existing = match read_optional_text(&path) {
        Ok(existing) => existing,
        Err(error) => return failed(ctx, &path, error),
    };

    let Some(existing) = existing else {
        if ctx.dry_run {
            return base.detail("create with an @AGENTS.md import");
        }
        let body = import_block();
        let content = format!(
            "{}
{body}",
            authored_marker_prefix_line(&normalized_for_hash(&body))
        );
        return match std::fs::write(&path, content) {
            Ok(()) => base.detail("created with an @AGENTS.md import"),
            Err(error) => failed(ctx, &path, error),
        };
    };

    if has_import_line(&existing) {
        return base.with_status(StepStatus::Unchanged);
    }
    let start = claude_import_start();
    let end = claude_import_end();
    if existing.contains(&start) || existing.contains(&end) {
        return base
            .with_status(StepStatus::Refused)
            .reason("managed_block_malformed")
            .detail(
                "fallow import markers exist without the import line; repair CLAUDE.md by hand",
            );
    }
    if ctx.dry_run {
        return base.detail("append an @AGENTS.md import block");
    }
    let mut next = existing;
    if !next.ends_with('\n') {
        next.push('\n');
    }
    next.push('\n');
    next.push_str(&import_block());
    match std::fs::write(&path, next) {
        Ok(()) => base.detail("appended an @AGENTS.md import block"),
        Err(error) => failed(ctx, &path, error),
    }
}

fn uninstall_agents_guide(ctx: &Ctx) -> StepReport {
    let path = ctx.root.join(AGENTS_FILE);
    let base =
        StepReport::new(None, Step::Guide, StepStatus::Removed, Scope::Shared).path(ctx, &path);
    let existing = match read_optional_text(&path) {
        Ok(Some(existing)) => existing,
        Ok(None) => {
            return base
                .with_status(StepStatus::Unchanged)
                .detail("not present");
        }
        Err(error) => return failed(ctx, &path, error),
    };
    if authored_unmodified(&existing) {
        if ctx.dry_run {
            return base.detail("delete the file fallow authored");
        }
        return match std::fs::remove_file(&path) {
            Ok(()) => base.detail("deleted the file fallow authored"),
            Err(error) => failed(ctx, &path, error),
        };
    }
    match remove_managed_block(&path, ctx.dry_run) {
        Ok(AgentsOutcome::Removed) => base.detail("fallow task map block removed; file kept"),
        Ok(AgentsOutcome::MalformedPreserved) => base
            .with_status(StepStatus::Refused)
            .reason("managed_block_malformed")
            .detail("fallow markers are out of order; repair AGENTS.md by hand"),
        Ok(_) => base.with_status(StepStatus::Unchanged),
        Err(error) => failed(ctx, &path, error),
    }
}

fn uninstall_claude_import(ctx: &Ctx) -> StepReport {
    let path = ctx.root.join(CLAUDE_FILE);
    let base = StepReport::new(
        Some(Harness::Claude),
        Step::Guide,
        StepStatus::Removed,
        Scope::Shared,
    )
    .path(ctx, &path);
    let existing = match read_optional_text(&path) {
        Ok(Some(existing)) => existing,
        Ok(None) => {
            return base
                .with_status(StepStatus::Unchanged)
                .detail("not present");
        }
        Err(error) => return failed(ctx, &path, error),
    };
    if authored_unmodified(&existing) {
        if ctx.dry_run {
            return base.detail("delete the file fallow authored");
        }
        return match std::fs::remove_file(&path) {
            Ok(()) => base.detail("deleted the file fallow authored"),
            Err(error) => failed(ctx, &path, error),
        };
    }
    let Some((start, end)) = import_block_bounds(&existing) else {
        return base.with_status(StepStatus::Unchanged);
    };
    if ctx.dry_run {
        return base.detail("remove the @AGENTS.md import block; file kept");
    }
    let mut next = String::with_capacity(existing.len());
    let prefix = existing[..start].trim_end_matches('\n');
    next.push_str(prefix);
    if !prefix.is_empty() {
        next.push('\n');
    }
    let tail = existing[end..].trim_start_matches('\n');
    next.push_str(tail);
    match std::fs::write(&path, next) {
        Ok(()) => base.detail("@AGENTS.md import block removed; file kept"),
        Err(error) => failed(ctx, &path, error),
    }
}

fn import_block() -> String {
    format!(
        "{}\n{IMPORT_LINE}\n{}\n",
        claude_import_start(),
        claude_import_end()
    )
}

/// Byte range of the Claude import block including its trailing newline.
fn import_block_bounds(text: &str) -> Option<(usize, usize)> {
    let start_marker = claude_import_start();
    let end_marker = claude_import_end();
    let start = text.find(&start_marker)?;
    let end = start + text[start..].find(&end_marker)? + end_marker.len();
    let end = text[end..]
        .find('\n')
        .map_or(text.len(), |offset| end + offset + 1);
    Some((start, end))
}

fn has_import_line(text: &str) -> bool {
    text.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == IMPORT_LINE || trimmed.starts_with("@AGENTS.md ")
    })
}

/// Content that participates in the authored hash: everything except the
/// marker line, the hooks managed block, and the Claude import block.
fn normalized_for_hash(text: &str) -> String {
    let mut body = text.to_string();
    if body.starts_with(&authored_marker_prefix()) {
        body = body
            .find('\n')
            .map_or(String::new(), |nl| body[nl + 1..].to_string());
    }
    if let Some(stripped) = strip_managed_block(&body) {
        body = stripped;
    }
    if let Some((start, end)) = import_block_bounds(&body) {
        body.replace_range(start..end, "");
    }
    debug_assert!(!body.contains(AGENTS_BLOCK_START));
    body
}

fn content_hash(normalized: &str) -> String {
    use std::fmt::Write as _;
    let digest = Sha256::digest(normalized.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

fn authored_marker_prefix_line(normalized: &str) -> String {
    format!(
        "{}{} -->",
        authored_marker_prefix(),
        content_hash(normalized)
    )
}

/// Prepend the authored marker to a file fallow just wrote whole.
fn stamp_authored(path: &Path) -> std::io::Result<()> {
    let text = std::fs::read_to_string(path)?;
    let normalized = normalized_for_hash(&text);
    let stamped = format!("{}\n{text}", authored_marker_prefix_line(&normalized));
    std::fs::write(path, stamped)
}

/// True when the file carries an authored marker and its content, minus
/// managed blocks, still hashes to the recorded value.
pub fn authored_unmodified(text: &str) -> bool {
    recorded_hash(text).is_some_and(|recorded| recorded == content_hash(&normalized_for_hash(text)))
}

/// True when the file starts with an authored marker, whatever its hash.
pub fn is_authored(text: &str) -> bool {
    recorded_hash(text).is_some()
}

fn recorded_hash(text: &str) -> Option<&str> {
    let rest = text.strip_prefix(&authored_marker_prefix())?;
    let end = rest.find(" -->")?;
    Some(&rest[..end])
}

impl StepReport {
    pub fn with_status(mut self, status: StepStatus) -> Self {
        self.status = status;
        self
    }
}

fn failed(ctx: &Ctx, path: &Path, error: impl std::fmt::Display) -> StepReport {
    StepReport::failed(None, Step::Guide, Scope::Shared, error.to_string()).path(ctx, path)
}
