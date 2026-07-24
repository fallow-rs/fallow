//! `fallow report --from <results.json>`: render an EXISTING fallow JSON
//! envelope in another format without re-running analysis (the analyze-once
//! flow: `fallow --format json -o results.json`, then one `report` call per
//! rendered surface).
//!
//! Supports the GitHub-native text formats plus CodeClimate and SARIF. Dispatch is on
//! the envelope's `kind` field, so any envelope produced by `--format json`
//! (dead-code, dupes, health, audit, security, or the bare combined run)
//! renders byte-identically to the direct `--format` run. The `fallow fix`
//! envelope carries no `kind`; it is detected by its top-level fields and
//! rendered via [`EnvelopeKind::Fix`].

use std::path::Path;
use std::process::ExitCode;

use fallow_config::{FallowConfig, OutputFormat};
use fallow_output::GroupByMode;

use crate::report::github_annotations::{self, EnvelopeKind};
use crate::report::github_summary;
use crate::telemetry;

/// Run `fallow report --from <file>` with the global `--format` and `--root`.
pub fn run_report(
    from: &Path,
    output: OutputFormat,
    root: &Path,
    config_path: Option<&Path>,
) -> ExitCode {
    if let Some(path) = config_path
        && let Err(error) = FallowConfig::load(path)
    {
        return crate::emit_known_failure(
            &format!("failed to load report config {}: {error}", path.display()),
            2,
            output,
            telemetry::FailureReason::Validation,
        );
    }
    let target = match output {
        OutputFormat::GithubAnnotations => ReportTarget::GithubAnnotations,
        OutputFormat::GithubSummary => ReportTarget::GithubSummary,
        OutputFormat::CodeClimate => ReportTarget::CodeClimate,
        OutputFormat::Sarif => ReportTarget::Sarif,
        _ => {
            return crate::emit_known_failure(
                "fallow report supports --format github-annotations, github-summary, codeclimate, or sarif only",
                2,
                output,
                telemetry::FailureReason::UnsupportedFormat,
            );
        }
    };
    let envelope = match load_envelope(from, output) {
        Ok(envelope) => envelope,
        Err(code) => return code,
    };
    let saved = normalize_saved_envelope(envelope);
    let kind = match envelope_kind(&saved.envelope, from, output) {
        Ok(kind) => kind,
        Err(code) => return code,
    };
    if matches!(target, ReportTarget::CodeClimate) && kind == EnvelopeKind::Security {
        return crate::emit_known_failure(
            "fallow security supports --format human, json, sarif, github-annotations, or github-summary only.",
            2,
            output,
            telemetry::FailureReason::UnsupportedFormat,
        );
    }
    let resolver = match saved_group_resolver(saved.grouped_by, root, config_path, output) {
        Ok(resolver) => resolver,
        Err(code) => return code,
    };
    match target {
        ReportTarget::GithubAnnotations => {
            github_annotations::print_annotations(kind, &saved.envelope, root)
        }
        ReportTarget::GithubSummary => github_summary::print_summary(kind, &saved.envelope, root),
        ReportTarget::CodeClimate => {
            crate::report::codeclimate::print_envelope_codeclimate_with_config(
                kind,
                &saved.envelope,
                root,
                config_path,
                resolver.as_ref(),
            )
        }
        ReportTarget::Sarif => crate::report::sarif::print_envelope_sarif_with_config(
            kind,
            &saved.envelope,
            root,
            config_path,
            resolver.as_ref(),
        ),
    }
}

struct SavedEnvelope {
    envelope: serde_json::Value,
    grouped_by: Option<GroupByMode>,
}

fn normalize_saved_envelope(mut envelope: serde_json::Value) -> SavedEnvelope {
    let grouped_by = envelope
        .get("grouped_by")
        .and_then(serde_json::Value::as_str)
        .and_then(parse_group_by_mode);
    if envelope.get("kind").and_then(serde_json::Value::as_str) != Some("dead-code-grouped") {
        return SavedEnvelope {
            envelope,
            grouped_by,
        };
    }
    let Some(root) = envelope.as_object_mut() else {
        return SavedEnvelope {
            envelope,
            grouped_by,
        };
    };
    let groups = root
        .remove("groups")
        .and_then(|groups| groups.as_array().cloned())
        .unwrap_or_default();
    root.remove("grouped_by");
    root.insert(
        "kind".to_string(),
        serde_json::Value::String("dead-code".to_string()),
    );
    for group in groups {
        let Some(group) = group.as_object() else {
            continue;
        };
        for (key, value) in group {
            if matches!(key.as_str(), "key" | "owners" | "total_issues") {
                continue;
            }
            let Some(items) = value.as_array() else {
                continue;
            };
            let target = root
                .entry(key.clone())
                .or_insert_with(|| serde_json::Value::Array(Vec::new()));
            if let Some(target) = target.as_array_mut() {
                target.extend(items.iter().cloned());
            }
        }
    }
    SavedEnvelope {
        envelope,
        grouped_by,
    }
}

fn parse_group_by_mode(value: &str) -> Option<GroupByMode> {
    match value {
        "owner" => Some(GroupByMode::Owner),
        "directory" => Some(GroupByMode::Directory),
        "package" => Some(GroupByMode::Package),
        "section" => Some(GroupByMode::Section),
        _ => None,
    }
}

fn saved_group_resolver(
    grouped_by: Option<GroupByMode>,
    root: &Path,
    config_path: Option<&Path>,
    output: OutputFormat,
) -> Result<Option<crate::report::OwnershipResolver>, ExitCode> {
    let codeowners = config_path
        .and_then(|path| FallowConfig::load(path).ok())
        .and_then(|config| config.codeowners)
        .or_else(|| {
            FallowConfig::find_and_load(root)
                .ok()
                .flatten()
                .and_then(|(config, _)| config.codeowners)
        });
    crate::runtime_support::build_ownership_resolver_for_mode(
        grouped_by,
        root,
        codeowners.as_deref(),
        output,
    )
}

#[derive(Clone, Copy)]
enum ReportTarget {
    GithubAnnotations,
    GithubSummary,
    CodeClimate,
    Sarif,
}

fn load_envelope(from: &Path, output: OutputFormat) -> Result<serde_json::Value, ExitCode> {
    let source = std::fs::read_to_string(from).map_err(|err| {
        crate::emit_known_failure(
            &format!("failed to read {}: {err}", from.display()),
            2,
            output,
            telemetry::FailureReason::Validation,
        )
    })?;
    serde_json::from_str(&source).map_err(|err| {
        crate::emit_known_failure(
            &format!(
                "{} is not valid JSON ({err}); generate it with `fallow ... --format json`",
                from.display()
            ),
            2,
            output,
            telemetry::FailureReason::Validation,
        )
    })
}

fn envelope_kind(
    envelope: &serde_json::Value,
    from: &Path,
    output: OutputFormat,
) -> Result<EnvelopeKind, ExitCode> {
    let Some(kind) = envelope.get("kind").and_then(serde_json::Value::as_str) else {
        // The `fallow fix --format json` envelope is the only kind-less document
        // fallow emits (crates/output/src/fix.rs: no top-level `kind`). Resolve it
        // by field detection so `report --from <fix-results.json>` renders the fix
        // job summary natively; genuinely unrecognized documents keep erroring.
        if is_fix_envelope(envelope) {
            return Ok(EnvelopeKind::Fix);
        }
        return Err(crate::emit_known_failure(
            &format!(
                "{} is not a fallow results envelope (missing top-level `kind`); \
                 generate it with `fallow ... --format json`",
                from.display()
            ),
            2,
            output,
            telemetry::FailureReason::Validation,
        ));
    };
    parse_envelope_kind(kind).ok_or_else(|| {
        crate::emit_known_failure(
            &format!(
                "unsupported envelope kind `{kind}` in {}; fallow report renders dead-code, \
                 dupes, health, audit, security, and combined envelopes",
                from.display()
            ),
            2,
            output,
            telemetry::FailureReason::Validation,
        )
    })
}

/// Map the `--format json` root `kind` onto the renderer dispatch. The fix
/// envelope has no `kind` field; it is resolved separately via
/// [`is_fix_envelope`] field detection (see [`envelope_kind`]).
fn parse_envelope_kind(kind: &str) -> Option<EnvelopeKind> {
    match kind {
        "dead-code" => Some(EnvelopeKind::DeadCode),
        "dupes" => Some(EnvelopeKind::Dupes),
        "health" => Some(EnvelopeKind::Health),
        "audit" => Some(EnvelopeKind::Audit),
        "security" => Some(EnvelopeKind::Security),
        "combined" => Some(EnvelopeKind::Combined),
        _ => None,
    }
}

/// Recognize a kind-less `fallow fix --format json` envelope by its stable
/// top-level keys. The fix root always carries both a `fixes` array and a
/// numeric `total_fixed` (see `crates/output/src/fix.rs::FixJsonOutput`); no
/// other fallow envelope is kind-less, so the two keys together are an
/// unambiguous signal.
fn is_fix_envelope(envelope: &serde_json::Value) -> bool {
    envelope
        .get("fixes")
        .is_some_and(serde_json::Value::is_array)
        && envelope
            .get("total_fixed")
            .is_some_and(serde_json::Value::is_number)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_envelope_kind_covers_supported_kinds() {
        assert_eq!(
            parse_envelope_kind("dead-code"),
            Some(EnvelopeKind::DeadCode)
        );
        assert_eq!(parse_envelope_kind("dupes"), Some(EnvelopeKind::Dupes));
        assert_eq!(parse_envelope_kind("health"), Some(EnvelopeKind::Health));
        assert_eq!(parse_envelope_kind("audit"), Some(EnvelopeKind::Audit));
        assert_eq!(
            parse_envelope_kind("security"),
            Some(EnvelopeKind::Security)
        );
        assert_eq!(
            parse_envelope_kind("combined"),
            Some(EnvelopeKind::Combined)
        );
    }

    #[test]
    fn parse_envelope_kind_rejects_unknown_and_grouped_kinds() {
        assert_eq!(parse_envelope_kind("dead-code-grouped"), None);
        assert_eq!(parse_envelope_kind("feature-flags"), None);
        assert_eq!(parse_envelope_kind(""), None);
    }

    #[test]
    fn grouped_dead_code_is_flattened_for_saved_renderers() {
        let normalized = normalize_saved_envelope(serde_json::json!({
            "kind": "dead-code-grouped",
            "grouped_by": "owner",
            "total_issues": 1,
            "groups": [{
                "key": "@team",
                "owners": ["@team"],
                "total_issues": 1,
                "unused_files": [{"path": "src/dead.ts", "actions": []}]
            }]
        }));

        assert_eq!(normalized.grouped_by, Some(GroupByMode::Owner));
        assert_eq!(normalized.envelope["kind"], "dead-code");
        assert_eq!(
            normalized.envelope["unused_files"][0]["path"],
            "src/dead.ts"
        );
        assert!(normalized.envelope.get("groups").is_none());
        assert!(normalized.envelope.get("grouped_by").is_none());
    }

    #[test]
    fn is_fix_envelope_detects_kindless_fix_document() {
        let fix = serde_json::json!({
            "dry_run": false,
            "total_fixed": 3,
            "skipped": 0,
            "fixes": [{ "type": "remove_export", "applied": true }],
        });
        assert!(is_fix_envelope(&fix));
    }

    #[test]
    fn is_fix_envelope_rejects_other_kindless_documents() {
        // A dead-code envelope stripped of its `kind` must NOT masquerade as
        // fix: it has neither `fixes` nor `total_fixed`.
        assert!(!is_fix_envelope(&serde_json::json!({
            "total_issues": 4,
            "unused_files": [{ "path": "src/a.ts" }],
        })));
        // `fixes` alone (no `total_fixed`) is not enough.
        assert!(!is_fix_envelope(&serde_json::json!({ "fixes": [] })));
        // `total_fixed` alone (no `fixes` array) is not enough.
        assert!(!is_fix_envelope(&serde_json::json!({ "total_fixed": 0 })));
        // A `fixes` value that is not an array is rejected.
        assert!(!is_fix_envelope(&serde_json::json!({
            "fixes": "nope",
            "total_fixed": 0,
        })));
    }
}
