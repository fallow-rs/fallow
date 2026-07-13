//! Instance-level conformance: REAL serializer output validated against the
//! published `docs/output-schema.json` (JSON Schema draft-07).
//!
//! This complements, and does not duplicate, the existing gates:
//!
//! - `crates/cli/src/bin/schema_emit.rs` + the CI "schema drift gate" prove the
//!   committed schema still matches the schemars-derived Rust types. That is a
//!   type <-> schema check; it never runs a real document through a validator.
//! - This test proves a REAL runtime document (built by the same `run_*` +
//!   `serialize_*_programmatic_json` path the MCP tools and the napi addon use)
//!   validates against that schema. It catches divergence a type-derivation
//!   gate cannot: post-pass injection (`attach_telemetry_meta`), envelope
//!   wrapping, and any hand-tuned serializer that drifts from its derived type.
//!   (The plan-028 Step 0 probe found exactly one such bug: feature-flags
//!   `_meta.telemetry`, fixed in `fix(output): conform feature-flags _meta`.)
//!
//! The schema is loaded from disk per-path (NOT `include_str!`) so the test
//! always reads the currently committed `docs/output-schema.json` without a
//! rebuild.
//!
//! Scope note (deliberate, see plan 028):
//! - Git-/pipeline-dependent envelopes (`audit`, `audit-brief`,
//!   `decision-surface`) are instance-validated end-to-end at the process level
//!   in `crates/cli/tests/schema_conformance.rs`, where they are actually
//!   assembled, rather than reconstructed in-process here.
//! - The programmatic trace family (`serialize_trace_*_programmatic_json`) is
//!   intentionally OUT of the published-envelope contract: it emits
//!   un-discriminated JSON with no `kind`, so it is not a `FallowOutput` oneOf
//!   branch. `trace_family_is_not_a_published_envelope` guards that boundary and
//!   records the follow-up (schematize the programmatic trace shapes, or keep
//!   them a separate programmatic-only contract).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests use unwrap/expect/panic to keep fixture setup and schema lookups concise"
)]

use std::path::{Path, PathBuf};

use fallow_api::{
    AnalysisOptions, CombinedOptions, ComplexityOptions, DeadCodeFilters, DeadCodeOptions,
    DuplicationOptions, FeatureFlagsOptions, TraceExportOptions, run_boundary_violations,
    run_circular_dependencies, run_combined, run_dead_code, run_duplication, run_feature_flags,
    run_health, run_trace_export, serialize_boundary_violations_programmatic_json,
    serialize_circular_dependencies_programmatic_json, serialize_combined_programmatic_json,
    serialize_dead_code_programmatic_json, serialize_duplication_programmatic_json,
    serialize_feature_flags_programmatic_json, serialize_health_programmatic_json,
    serialize_trace_export_programmatic_json,
};
use serde_json::Value;

/// Path to the committed schema, resolved from this crate's manifest dir so the
/// test tracks the real `docs/output-schema.json` without a rebuild.
fn schema_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/output-schema.json")
}

/// Loads `docs/output-schema.json` and compiles a per-`kind` draft-07 validator
/// on demand from the matching `FallowOutput` oneOf branch.
///
/// Validating against the single branch (not the whole 26-way oneOf) yields
/// readable, scoped error paths and makes a vacuous pass impossible: the branch
/// carries the `kind` const, so a document with the wrong `kind` cannot satisfy
/// it.
struct EnvelopeSchema {
    root: Value,
}

impl EnvelopeSchema {
    fn load() -> Self {
        let text = std::fs::read_to_string(schema_path()).expect("read output-schema.json");
        let root = serde_json::from_str(&text).expect("parse output-schema.json");
        Self { root }
    }

    fn definitions(&self) -> &Value {
        self.root
            .get("definitions")
            .expect("schema has a definitions object")
    }

    /// Build a self-contained draft-07 schema for a single `FallowOutput`
    /// branch: the branch's `allOf` plus the full `definitions` map so internal
    /// `#/definitions/...` refs resolve.
    fn branch_schema(&self, kind: &str) -> Value {
        let branches = self
            .definitions()
            .get("FallowOutput")
            .and_then(|f| f.get("oneOf"))
            .and_then(Value::as_array)
            .expect("FallowOutput.oneOf is an array");

        let branch = branches
            .iter()
            .find(|branch| branch_matches_kind(branch, kind))
            .unwrap_or_else(|| panic!("no FallowOutput oneOf branch declares kind {kind:?}"));

        serde_json::json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "allOf": branch.get("allOf").expect("branch has an allOf"),
            "definitions": self.definitions(),
        })
    }

    /// Assert `value` conforms to the schema branch for `kind`, and that its own
    /// `kind` discriminator matches (belt-and-suspenders against a vacuous pass).
    fn assert_conforms(&self, kind: &str, value: &Value) {
        assert_eq!(
            value.get("kind").and_then(Value::as_str),
            Some(kind),
            "document kind must be {kind:?}, got {:?}",
            value.get("kind"),
        );

        let schema = self.branch_schema(kind);
        let validator = jsonschema::draft7::new(&schema)
            .unwrap_or_else(|err| panic!("compile draft-07 branch for {kind:?}: {err}"));

        let errors: Vec<String> = validator
            .iter_errors(value)
            .map(|err| format!("  at {} : {err}", err.instance_path()))
            .collect();

        assert!(
            errors.is_empty(),
            "'{kind}' document does not conform to docs/output-schema.json:\n{}",
            errors.join("\n"),
        );
    }
}

/// Does this oneOf branch pin `kind` to `const == kind`?
fn branch_matches_kind(branch: &Value, kind: &str) -> bool {
    branch
        .get("allOf")
        .and_then(Value::as_array)
        .is_some_and(|parts| {
            parts.iter().any(|part| {
                part.get("properties")
                    .and_then(|p| p.get("kind"))
                    .and_then(|k| k.get("const"))
                    .and_then(Value::as_str)
                    == Some(kind)
            })
        })
}

fn analysis_at(root: &Path) -> AnalysisOptions {
    AnalysisOptions {
        root: Some(root.to_path_buf()),
        no_cache: true,
        ..AnalysisOptions::default()
    }
}

/// A tiny project with one entry, two dead exports, and one import edge: enough
/// to exercise the dead-code, duplication, health, and combined serializers.
fn small_project() -> tempfile::TempDir {
    let project = tempfile::tempdir().expect("temp dir");
    let root = project.path();
    std::fs::create_dir(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"conformance-fixture","main":"src/index.ts"}"#,
    )
    .expect("package.json");
    std::fs::write(
        root.join("src/index.ts"),
        "import './lib';\nexport const entry = 1;\nconsole.log(entry);\n",
    )
    .expect("entry");
    std::fs::write(
        root.join("src/lib.ts"),
        "export const used = 1;\nexport const deadA = 2;\nexport const deadB = 3;\n",
    )
    .expect("lib");
    project
}

#[test]
fn dead_code_document_conforms() {
    let project = small_project();
    let run = run_dead_code(&DeadCodeOptions {
        analysis: analysis_at(project.path()),
        filters: DeadCodeFilters {
            unused_exports: true,
            ..DeadCodeFilters::default()
        },
        ..DeadCodeOptions::default()
    })
    .expect("dead-code runs");
    let json = serialize_dead_code_programmatic_json(run).expect("serialize dead-code");
    EnvelopeSchema::load().assert_conforms("dead-code", &json);
}

#[test]
fn duplication_document_conforms() {
    let project = small_project();
    let run = run_duplication(&DuplicationOptions {
        analysis: analysis_at(project.path()),
        ..DuplicationOptions::default()
    })
    .expect("duplication runs");
    let json = serialize_duplication_programmatic_json(run).expect("serialize duplication");
    EnvelopeSchema::load().assert_conforms("dupes", &json);
}

#[test]
fn feature_flags_document_conforms() {
    let project = tempfile::tempdir().expect("temp dir");
    let root = project.path();
    std::fs::create_dir(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"flags-fixture","main":"src/index.ts"}"#,
    )
    .expect("package.json");
    std::fs::write(
        root.join("src/index.ts"),
        "if (process.env.FEATURE_ALPHA) {\n  console.log('on');\n}\n",
    )
    .expect("entry");

    // Default path (no --explain): _meta carries only telemetry. This is the
    // exact shape that failed before the FeatureFlagsMeta schema fix.
    let run = run_feature_flags(&FeatureFlagsOptions {
        analysis: analysis_at(root),
        top: None,
    })
    .expect("feature-flags runs");
    let json = serialize_feature_flags_programmatic_json(run).expect("serialize feature-flags");
    EnvelopeSchema::load().assert_conforms("feature-flags", &json);
}

#[test]
fn feature_flags_explain_document_conforms() {
    let project = tempfile::tempdir().expect("temp dir");
    let root = project.path();
    std::fs::create_dir(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"flags-explain-fixture","main":"src/index.ts"}"#,
    )
    .expect("package.json");
    std::fs::write(
        root.join("src/index.ts"),
        "if (process.env.FEATURE_ALPHA) {\n  console.log('on');\n}\n",
    )
    .expect("entry");

    // --explain path: _meta carries both feature_flags AND telemetry.
    let run = run_feature_flags(&FeatureFlagsOptions {
        analysis: AnalysisOptions {
            explain: true,
            ..analysis_at(root)
        },
        top: None,
    })
    .expect("feature-flags --explain runs");
    let json = serialize_feature_flags_programmatic_json(run).expect("serialize feature-flags");
    assert!(
        json["_meta"]["feature_flags"].is_object(),
        "--explain must populate _meta.feature_flags"
    );
    EnvelopeSchema::load().assert_conforms("feature-flags", &json);
}

#[test]
fn health_document_conforms() {
    let project = small_project();
    let run = run_health(&ComplexityOptions {
        analysis: analysis_at(project.path()),
        complexity: true,
        score: true,
        ..ComplexityOptions::default()
    })
    .expect("health runs");
    let json = serialize_health_programmatic_json(run).expect("serialize health");
    EnvelopeSchema::load().assert_conforms("health", &json);
}

#[test]
fn combined_document_conforms() {
    let project = small_project();
    let run = run_combined(&CombinedOptions {
        analysis: analysis_at(project.path()),
        health_options: ComplexityOptions {
            complexity: true,
            score: true,
            ..ComplexityOptions::default()
        },
        ..CombinedOptions::default()
    })
    .expect("combined runs");
    let json = serialize_combined_programmatic_json(run).expect("serialize combined");
    EnvelopeSchema::load().assert_conforms("combined", &json);
}

#[test]
fn circular_dependencies_document_conforms_as_dead_code() {
    // run_circular_dependencies serializes a filtered CheckOutput; the wire
    // discriminator is `dead-code`, not a distinct kind (the "circular" string
    // is only an error context). Validate against the dead-code branch.
    let project = small_project();
    let run = run_circular_dependencies(&DeadCodeOptions {
        analysis: analysis_at(project.path()),
        ..DeadCodeOptions::default()
    })
    .expect("circular-dependencies runs");
    let json =
        serialize_circular_dependencies_programmatic_json(run).expect("serialize circular deps");
    EnvelopeSchema::load().assert_conforms("dead-code", &json);
}

#[test]
fn boundary_violations_document_conforms_as_dead_code() {
    let project = small_project();
    let run = run_boundary_violations(&DeadCodeOptions {
        analysis: analysis_at(project.path()),
        ..DeadCodeOptions::default()
    })
    .expect("boundary-violations runs");
    let json = serialize_boundary_violations_programmatic_json(run)
        .expect("serialize boundary violations");
    EnvelopeSchema::load().assert_conforms("dead-code", &json);
}

/// The programmatic trace serializers are a separate, un-enveloped contract:
/// they emit no `kind` and are not a `FallowOutput` oneOf branch, so they are
/// out of the published-envelope schema by design (plan-028 ruling). This guard
/// fails loudly if a trace serializer ever grows a root `kind`, which would
/// force a deliberate schema decision (schematize the trace shapes, or keep them
/// programmatic-only). Follow-up: decide whether to add trace shapes to the
/// envelope schema.
#[test]
fn trace_family_is_not_a_published_envelope() {
    let project = small_project();
    let out = run_trace_export(&TraceExportOptions {
        analysis: analysis_at(project.path()),
        file: "src/lib.ts".to_string(),
        export_name: "deadA".to_string(),
    })
    .expect("trace export runs");
    let json = serialize_trace_export_programmatic_json(out).expect("serialize trace export");

    assert!(
        json.get("kind").is_none(),
        "programmatic trace output must stay un-enveloped (no root kind); if this \
         changes, schematize the trace shapes in docs/output-schema.json first. Got: {json}"
    );
}
