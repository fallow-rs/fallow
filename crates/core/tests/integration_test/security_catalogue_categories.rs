//! Integration tests for the data-driven security matcher catalogue categories
//! beyond `dangerous-html` (which is covered in `security_dangerous_html.rs`).
//!
//! Every shipped catalogue category that can fire at runtime is exercised here
//! with a positive case (a non-literal sink fires exactly one candidate of the
//! right category + CWE) and a negative case (a literal/safe argument fires
//! nothing), mirroring the `dangerous-html` fixture shape. Each category also
//! confirms the default-off behavior: with the `security_sink` rule at its
//! default `off`, no tainted-sink finding is produced.
//!
//! Findings are CANDIDATES for downstream agent verification, NOT verified
//! vulnerabilities.

use fallow_config::Severity;
use fallow_core::results::{AnalysisResults, SecurityFinding, SecurityFindingKind};

use super::common::{create_config, create_config_with_rules, fixture_path};

fn analyze_with_security_sink(fixture: &str) -> AnalysisResults {
    let root = fixture_path(fixture);
    let config = create_config_with_rules(root, |rules| {
        rules.security_sink = Severity::Warn;
    });
    fallow_core::analyze(&config).expect("analysis should succeed")
}

fn analyze_default_off(fixture: &str) -> AnalysisResults {
    let root = fixture_path(fixture);
    let config = create_config(root);
    assert_eq!(config.rules.security_sink, Severity::Off);
    fallow_core::analyze(&config).expect("analysis should succeed")
}

/// Find a tainted-sink candidate anchored on a path suffix and assert it carries
/// the expected category + CWE plus a suppress action.
fn assert_candidate(results: &AnalysisResults, suffix: &str, category: &str, cwe: u32) {
    let finding = results
        .security_findings
        .iter()
        .find(|f| {
            f.path
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with(suffix)
        })
        .unwrap_or_else(|| panic!("{suffix} should produce a {category} candidate"));
    assert!(
        matches!(finding.kind, SecurityFindingKind::TaintedSink),
        "{suffix} should be a tainted-sink candidate"
    );
    assert_eq!(
        finding.category.as_deref(),
        Some(category),
        "category for {suffix}"
    );
    assert_eq!(finding.cwe, Some(cwe), "cwe for {suffix}");
    assert!(
        !finding.actions.is_empty(),
        "candidate {suffix} must carry a suppress action"
    );
}

fn anchored_on(results: &AnalysisResults, suffix: &str) -> bool {
    results.security_findings.iter().any(|f| {
        f.path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with(suffix)
    })
}

fn category_count(results: &AnalysisResults, category: &str) -> usize {
    results
        .security_findings
        .iter()
        .filter(|finding| finding.category.as_deref() == Some(category))
        .count()
}

fn anchored_count(results: &AnalysisResults, suffix: &str) -> usize {
    results
        .security_findings
        .iter()
        .filter(|f| {
            f.path
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with(suffix)
        })
        .count()
}

fn finding_on<'a>(results: &'a AnalysisResults, suffix: &str) -> &'a SecurityFinding {
    results
        .security_findings
        .iter()
        .find(|f| {
            f.path
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with(suffix)
        })
        .unwrap_or_else(|| panic!("{suffix} should produce a security finding"))
}

fn no_tainted_sinks(results: &AnalysisResults) -> bool {
    results
        .security_findings
        .iter()
        .all(|f| !matches!(f.kind, SecurityFindingKind::TaintedSink))
}

// ── command-injection (CWE-78), provenance-gated to node:child_process ───────

#[test]
fn command_injection_non_literal_fires() {
    let results = analyze_with_security_sink("security-command-injection");
    assert_candidate(&results, "src/sink.ts", "command-injection", 78);
}

#[test]
fn command_injection_literal_does_not_fire() {
    let results = analyze_with_security_sink("security-command-injection");
    assert!(
        !anchored_on(&results, "src/safe.ts"),
        "a literal command must not be flagged"
    );
}

#[test]
fn command_injection_without_provenance_does_not_fire() {
    // A same-named local `exec` that does NOT come from node:child_process must
    // not fire: the matcher is binding-traced (false-negative preferred).
    let results = analyze_with_security_sink("security-command-injection");
    assert!(
        !anchored_on(&results, "src/no-provenance.ts"),
        "a same-named local exec without node:child_process provenance must not fire"
    );
}

#[test]
fn command_injection_default_off_emits_nothing() {
    assert!(no_tainted_sinks(&analyze_default_off(
        "security-command-injection"
    )));
}

// ── code-injection (CWE-94): eval (ungated) + vm (node:vm) ───────────────────

#[test]
fn code_injection_eval_non_literal_fires() {
    let results = analyze_with_security_sink("security-code-injection");
    assert_candidate(&results, "src/sink.ts", "code-injection", 94);
}

#[test]
fn code_injection_vm_non_literal_fires() {
    let results = analyze_with_security_sink("security-code-injection");
    assert_candidate(&results, "src/vm.ts", "code-injection", 94);
}

#[test]
fn code_injection_new_function_non_literal_fires() {
    let results = analyze_with_security_sink("security-code-injection");
    assert_candidate(&results, "src/new-function.ts", "code-injection", 95);
}

#[test]
fn code_injection_literal_does_not_fire() {
    let results = analyze_with_security_sink("security-code-injection");
    assert!(
        !anchored_on(&results, "src/safe.ts"),
        "a literal eval argument must not be flagged"
    );
}

#[test]
fn code_injection_default_off_emits_nothing() {
    assert!(no_tainted_sinks(&analyze_default_off(
        "security-code-injection"
    )));
}

// ── dynamic-regex (CWE-1333), ungated non-literal pattern construction ──────

#[test]
fn dynamic_regex_call_non_literal_fires() {
    let results = analyze_with_security_sink("security-dynamic-regex");
    assert_candidate(&results, "src/call.ts", "dynamic-regex", 1333);
}

#[test]
fn dynamic_regex_new_expression_non_literal_fires() {
    let results = analyze_with_security_sink("security-dynamic-regex");
    assert_candidate(&results, "src/constructor.ts", "dynamic-regex", 1333);
}

#[test]
fn dynamic_regex_literal_patterns_do_not_fire() {
    let results = analyze_with_security_sink("security-dynamic-regex");
    assert!(
        !anchored_on(&results, "src/safe.ts"),
        "literal regex patterns must not be flagged"
    );
}

#[test]
fn dynamic_regex_source_backed_candidate_is_marked() {
    let results = analyze_with_security_sink("security-dynamic-regex");
    let finding = finding_on(&results, "src/source-backed.ts");

    assert_eq!(finding.category.as_deref(), Some("dynamic-regex"));
    assert!(
        finding.source_backed,
        "request-sourced regex pattern should carry the ranking signal"
    );
}

#[test]
fn dynamic_regex_one_hop_function_helper_source_backed_candidate_is_marked() {
    let results = analyze_with_security_sink("security-dynamic-regex");
    let finding = finding_on(&results, "src/source-helper.ts");

    assert_eq!(finding.category.as_deref(), Some("dynamic-regex"));
    assert!(
        finding.source_backed,
        "one-hop function helper result should carry the ranking signal"
    );
}

#[test]
fn dynamic_regex_hoisted_function_helper_source_backed_candidate_is_marked() {
    let results = analyze_with_security_sink("security-dynamic-regex");
    let finding = finding_on(&results, "src/source-helper-hoisted.ts");

    assert_eq!(finding.category.as_deref(), Some("dynamic-regex"));
    assert!(
        finding.source_backed,
        "hoisted function helper result should carry the ranking signal"
    );
}

#[test]
fn dynamic_regex_one_hop_arrow_helper_source_backed_candidate_is_marked() {
    let results = analyze_with_security_sink("security-dynamic-regex");
    let finding = finding_on(&results, "src/source-helper-arrow.ts");

    assert_eq!(finding.category.as_deref(), Some("dynamic-regex"));
    assert!(
        finding.source_backed,
        "one-hop arrow helper result should carry the ranking signal"
    );
}

#[test]
fn dynamic_regex_one_hop_function_expression_source_backed_candidate_is_marked() {
    let results = analyze_with_security_sink("security-dynamic-regex");
    let finding = finding_on(&results, "src/source-helper-function-expression.ts");

    assert_eq!(finding.category.as_deref(), Some("dynamic-regex"));
    assert!(
        finding.source_backed,
        "one-hop function expression helper result should carry the ranking signal"
    );
}

#[test]
fn dynamic_regex_shadowed_helper_name_keeps_candidate_unbacked() {
    let results = analyze_with_security_sink("security-dynamic-regex");
    let finding = finding_on(&results, "src/source-helper-shadowed.ts");

    assert_eq!(finding.category.as_deref(), Some("dynamic-regex"));
    assert!(
        !finding.source_backed,
        "a local parameter must shadow the same-named module helper"
    );
}

#[test]
fn dynamic_regex_second_hop_helper_keeps_candidate_unbacked() {
    let results = analyze_with_security_sink("security-dynamic-regex");
    let finding = finding_on(&results, "src/source-helper-multihop.ts");

    assert_eq!(finding.category.as_deref(), Some("dynamic-regex"));
    assert!(
        !finding.source_backed,
        "helper chains beyond one call are out of scope"
    );
}

#[test]
fn dynamic_regex_default_off_emits_nothing() {
    assert!(no_tainted_sinks(&analyze_default_off(
        "security-dynamic-regex"
    )));
}

// ── sql-injection (CWE-89), ungated (broad tier) ─────────────────────────────

#[test]
fn sql_injection_concat_fires() {
    // A string-concatenation argument into db.query() is the unsafe shape.
    let results = analyze_with_security_sink("security-sql-injection");
    assert_candidate(&results, "src/sink.ts", "sql-injection", 89);
}

#[test]
fn sql_injection_raw_fires() {
    // Drizzle's sql.raw(<non-literal>) bypasses parameterization.
    let results = analyze_with_security_sink("security-sql-injection");
    assert_candidate(&results, "src/raw.ts", "sql-injection", 89);
}

#[test]
fn sql_injection_literal_does_not_fire() {
    let results = analyze_with_security_sink("security-sql-injection");
    assert!(
        !anchored_on(&results, "src/safe.ts"),
        "a literal SQL string must not be flagged"
    );
}

#[test]
fn sql_injection_parameterized_template_and_object_do_not_fire() {
    // A bare parameterized `sql`${x}`` tagged template binds safely (no bare-sql
    // matcher row), and the object-form `.execute({ sql, args })` is the
    // parameterized shape excluded by arg_kinds. Neither must fire.
    let results = analyze_with_security_sink("security-sql-injection");
    assert!(
        !anchored_on(&results, "src/parameterized.ts"),
        "a parameterized sql template / object-form execute must not be flagged"
    );
}

#[test]
fn sql_injection_default_off_emits_nothing() {
    assert!(no_tainted_sinks(&analyze_default_off(
        "security-sql-injection"
    )));
}

// ── ssrf (CWE-918), ungated (broad tier) ─────────────────────────────────────

#[test]
fn ssrf_non_literal_fires() {
    let results = analyze_with_security_sink("security-ssrf");
    assert_candidate(&results, "src/sink.ts", "ssrf", 918);
}

#[test]
fn ssrf_literal_does_not_fire() {
    let results = analyze_with_security_sink("security-ssrf");
    assert!(
        !anchored_on(&results, "src/safe.ts"),
        "a literal fetch URL must not be flagged"
    );
}

#[test]
fn ssrf_literal_allowlist_guard_does_not_fire() {
    let results = analyze_with_security_sink("security-ssrf");
    assert!(
        !anchored_on(&results, "src/allowlisted.ts"),
        "a URL checked against a literal-backed fail-closed allowlist must not be flagged"
    );
}

#[test]
fn ssrf_helper_predicate_still_fires() {
    let results = analyze_with_security_sink("security-ssrf");
    assert_candidate(&results, "src/helper-predicate.ts", "ssrf", 918);
}

#[test]
fn ssrf_default_off_emits_nothing() {
    assert!(no_tainted_sinks(&analyze_default_off("security-ssrf")));
}

// ── path-traversal (CWE-22), provenance-gated to node:path ───────────────────

#[test]
fn path_traversal_non_literal_fires() {
    let results = analyze_with_security_sink("security-path-traversal");
    assert_candidate(&results, "src/sink.ts", "path-traversal", 22);
}

#[test]
fn path_traversal_literal_does_not_fire() {
    let results = analyze_with_security_sink("security-path-traversal");
    assert!(
        !anchored_on(&results, "src/safe.ts"),
        "a fully-literal path.join must not be flagged"
    );
}

#[test]
fn path_traversal_relative_containment_guard_does_not_fire() {
    let results = analyze_with_security_sink("security-path-traversal");
    assert!(
        !anchored_on(&results, "src/contained.ts"),
        "a path.relative containment guard must suppress the path candidate"
    );
}

#[test]
fn path_traversal_prefix_collision_check_still_fires() {
    let results = analyze_with_security_sink("security-path-traversal");
    assert_candidate(&results, "src/prefix-collision.ts", "path-traversal", 22);
}

#[test]
fn path_traversal_post_guard_still_fires() {
    let results = analyze_with_security_sink("security-path-traversal");
    assert_candidate(&results, "src/post-guard.ts", "route-send-file", 22);
}

#[test]
fn path_traversal_url_guard_still_fires() {
    let results = analyze_with_security_sink("security-path-traversal");
    assert_candidate(&results, "src/url-guard.ts", "path-traversal", 22);
}

#[test]
fn path_traversal_default_off_emits_nothing() {
    assert!(no_tainted_sinks(&analyze_default_off(
        "security-path-traversal"
    )));
}

// ── open-redirect (CWE-601), ungated (broad tier) ────────────────────────────

#[test]
fn open_redirect_non_literal_fires() {
    let results = analyze_with_security_sink("security-open-redirect");
    assert_candidate(&results, "src/sink.ts", "open-redirect", 601);
}

#[test]
fn open_redirect_literal_does_not_fire() {
    let results = analyze_with_security_sink("security-open-redirect");
    assert!(
        !anchored_on(&results, "src/safe.ts"),
        "a literal redirect target must not be flagged"
    );
}

#[test]
fn open_redirect_literal_allowlist_guard_does_not_fire() {
    let results = analyze_with_security_sink("security-open-redirect");
    assert!(
        !anchored_on(&results, "src/allowlisted.ts"),
        "a redirect checked against a literal-backed fail-closed allowlist must not be flagged"
    );
}

#[test]
fn open_redirect_mutable_allowlist_still_fires() {
    let results = analyze_with_security_sink("security-open-redirect");
    assert_candidate(&results, "src/mutable-allowlist.ts", "open-redirect", 601);
}

#[test]
fn open_redirect_mutated_const_allowlist_still_fires() {
    let results = analyze_with_security_sink("security-open-redirect");
    assert_candidate(
        &results,
        "src/mutated-const-allowlist.ts",
        "open-redirect",
        601,
    );
}

#[test]
fn open_redirect_reassigned_guarded_target_still_fires() {
    let results = analyze_with_security_sink("security-open-redirect");
    assert_candidate(&results, "src/reassigned-target.ts", "open-redirect", 601);
}

#[test]
fn open_redirect_post_guard_still_fires() {
    let results = analyze_with_security_sink("security-open-redirect");
    assert_candidate(&results, "src/post-guard.ts", "open-redirect", 601);
}

#[test]
fn open_redirect_default_off_emits_nothing() {
    assert!(no_tainted_sinks(&analyze_default_off(
        "security-open-redirect"
    )));
}

// ── weak-crypto (CWE-327), provenance-gated to node:crypto ───────────────────

#[test]
fn weak_crypto_non_literal_fires() {
    let results = analyze_with_security_sink("security-weak-crypto");
    assert_candidate(&results, "src/sink.ts", "weak-crypto", 327);
}

#[test]
fn weak_crypto_strong_literal_does_not_fire() {
    let results = analyze_with_security_sink("security-weak-crypto");
    assert!(
        !anchored_on(&results, "src/safe.ts"),
        "a strong literal crypto algorithm must not be flagged"
    );
}

#[test]
fn weak_crypto_default_off_emits_nothing() {
    assert!(no_tainted_sinks(&analyze_default_off(
        "security-weak-crypto"
    )));
}

// ── unsafe-deserialization (CWE-502): js-yaml + node-serialize ───────────────

#[test]
fn unsafe_deserialization_yaml_non_literal_fires() {
    let results = analyze_with_security_sink("security-unsafe-deserialization");
    assert_candidate(&results, "src/sink.ts", "unsafe-deserialization", 502);
}

#[test]
fn unsafe_deserialization_node_serialize_non_literal_fires() {
    let results = analyze_with_security_sink("security-unsafe-deserialization");
    assert_candidate(&results, "src/serialize.ts", "unsafe-deserialization", 502);
}

#[test]
fn unsafe_deserialization_literal_does_not_fire() {
    let results = analyze_with_security_sink("security-unsafe-deserialization");
    assert!(
        !anchored_on(&results, "src/safe.ts"),
        "a literal yaml.load input must not be flagged"
    );
}

#[test]
fn unsafe_deserialization_default_off_emits_nothing() {
    assert!(no_tainted_sinks(&analyze_default_off(
        "security-unsafe-deserialization"
    )));
}

// ── prototype-pollution (CWE-1321), ungated (broad tier) ─────────────────────

#[test]
fn prototype_pollution_non_literal_merge_fires() {
    // A recursive merge of a non-literal (variable) source is the pollution shape
    // (sink.ts line 7).
    let results = analyze_with_security_sink("security-prototype-pollution");
    assert_candidate(&results, "src/sink.ts", "prototype-pollution", 1321);
}

#[test]
fn prototype_pollution_static_proto_assign_fires() {
    // A direct static `obj.__proto__ = <non-literal>` member-assign (sink.ts line 14)
    // flattens to the `*.__proto__` matcher. Asserted by line so it cannot be
    // satisfied by the merge candidate alone.
    let results = analyze_with_security_sink("security-prototype-pollution");
    assert!(
        results.security_findings.iter().any(|f| {
            f.path
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with("src/sink.ts")
                && f.category.as_deref() == Some("prototype-pollution")
                && f.line == 14
        }),
        "a static `obj.__proto__ = x` assign must produce a prototype-pollution candidate at line 14"
    );
}

#[test]
fn prototype_pollution_safe_forms_do_not_fire() {
    // Two negatives in src/safe.ts must both stay silent: an inline object-literal
    // merge source (the `object` arg shape, excluded), and a TypeScript-cast
    // `(obj as {...}).__proto__ = x` assign, which is a documented flattening blind
    // spot (the cast object is not a bare identifier, so the callee path does not
    // resolve to `*.__proto__`). The second is a conservative false-negative, not a
    // safe pattern; this assertion flips if a future flattening change starts
    // matching the cast form.
    let results = analyze_with_security_sink("security-prototype-pollution");
    assert!(
        !anchored_on(&results, "src/safe.ts"),
        "neither an inline-object merge nor a cast-form __proto__ assign may be flagged"
    );
}

#[test]
fn prototype_pollution_default_off_emits_nothing() {
    assert!(no_tainted_sinks(&analyze_default_off(
        "security-prototype-pollution"
    )));
}

// ── zip-slip / tar path traversal (CWE-22), ungated (broad tier) ─────────────

#[test]
fn zip_slip_non_literal_dest_fires() {
    let results = analyze_with_security_sink("security-zip-slip");
    assert_candidate(&results, "src/sink.ts", "zip-slip", 22);
}

#[test]
fn zip_slip_literal_dest_does_not_fire() {
    let results = analyze_with_security_sink("security-zip-slip");
    assert!(
        !anchored_on(&results, "src/safe.ts"),
        "a fully-literal extraction destination must not be flagged"
    );
}

#[test]
fn zip_slip_default_off_emits_nothing() {
    assert!(no_tainted_sinks(&analyze_default_off("security-zip-slip")));
}

// ── nosql-injection (CWE-943), ungated (broad tier) ──────────────────────────

#[test]
fn nosql_injection_passthrough_object_fires() {
    // A whole user-controlled filter passed through to a Mongo-specific verb
    // (`findOne`, the `other` shape) fires.
    let results = analyze_with_security_sink("security-nosql-injection");
    assert_candidate(&results, "src/sink.ts", "nosql-injection", 943);
}

#[test]
fn nosql_injection_safe_forms_do_not_fire() {
    // Two negatives in src/safe.ts must both stay silent: an inline object-literal
    // filter (the `object` shape, excluded) and `Array.prototype.find(callback)`
    // (the `*.find` pattern is deliberately dropped so array iteration is never
    // mistaken for a Mongo query).
    let results = analyze_with_security_sink("security-nosql-injection");
    assert!(
        !anchored_on(&results, "src/safe.ts"),
        "neither an inline-object filter nor Array.prototype.find may be flagged"
    );
}

#[test]
fn nosql_injection_default_off_emits_nothing() {
    assert!(no_tainted_sinks(&analyze_default_off(
        "security-nosql-injection"
    )));
}

// ── ssti (CWE-1336), ungated (broad tier) ────────────────────────────────────

#[test]
fn ssti_non_literal_template_fires() {
    let results = analyze_with_security_sink("security-ssti");
    assert_candidate(&results, "src/sink.ts", "ssti", 1336);
}

#[test]
fn ssti_literal_template_does_not_fire() {
    let results = analyze_with_security_sink("security-ssti");
    assert!(
        !anchored_on(&results, "src/safe.ts"),
        "a fully-literal template source must not be flagged"
    );
}

#[test]
fn ssti_default_off_emits_nothing() {
    assert!(no_tainted_sinks(&analyze_default_off("security-ssti")));
}

// ── xxe (CWE-611), ungated (broad tier) ──────────────────────────────────────

#[test]
fn xxe_non_literal_document_fires() {
    let results = analyze_with_security_sink("security-xxe");
    assert_candidate(&results, "src/sink.ts", "xxe", 611);
}

#[test]
fn xxe_literal_document_does_not_fire() {
    let results = analyze_with_security_sink("security-xxe");
    assert!(
        !anchored_on(&results, "src/safe.ts"),
        "a fully-literal XML document must not be flagged"
    );
}

#[test]
fn xxe_default_off_emits_nothing() {
    assert!(no_tainted_sinks(&analyze_default_off("security-xxe")));
}

// ── issue #882 catalogue-only sinks ─────────────────────────────────────────

#[test]
fn issue_882_catalogue_sinks_fire() {
    let results = analyze_with_security_sink("security-catalogue-sinks-882");
    assert_candidate(
        &results,
        "src/dynamic-module-load.ts",
        "dynamic-module-load",
        95,
    );
    assert_candidate(&results, "src/fs-path.ts", "path-traversal", 22);
    assert_candidate(&results, "src/header-injection.ts", "header-injection", 113);
    assert_candidate(&results, "src/raw-sql-escape.ts", "sql-injection", 89);
    assert_candidate(&results, "src/dom-navigation.ts", "open-redirect", 601);
    assert_candidate(&results, "src/mass-assignment.ts", "mass-assignment", 915);
    assert_candidate(&results, "src/ssrf-clients.ts", "ssrf", 918);
}

#[test]
fn issue_882_literals_and_source_free_mass_assignment_do_not_fire() {
    let results = analyze_with_security_sink("security-catalogue-sinks-882");
    assert!(
        !anchored_on(&results, "src/safe.ts"),
        "literal sinks and source-free mass assignment must not be flagged"
    );
}

#[test]
fn issue_882_default_off_emits_nothing() {
    assert!(no_tainted_sinks(&analyze_default_off(
        "security-catalogue-sinks-882"
    )));
}

// ── secret-pii-log (CWE-532), source-backed logging only ────────────────────

#[test]
fn secret_pii_log_source_backed_logs_fire() {
    let results = analyze_with_security_sink("security-secret-pii-log");
    assert_candidate(&results, "src/sink.ts", "secret-pii-log", 532);
    assert_eq!(
        category_count(&results, "secret-pii-log"),
        4,
        "request body locals, destructured body fields, env locals, and direct env expressions should all fire"
    );
    assert!(
        results
            .security_findings
            .iter()
            .filter(|finding| finding.category.as_deref() == Some("secret-pii-log"))
            .all(|finding| finding.source_backed),
        "logging candidates must be source-backed"
    );
}

#[test]
fn secret_pii_log_literals_and_source_free_logs_do_not_fire() {
    let results = analyze_with_security_sink("security-secret-pii-log");
    assert!(
        !anchored_on(&results, "src/safe.ts"),
        "literal logs and source-free logger calls must not be flagged"
    );
}

#[test]
fn secret_pii_log_default_off_emits_nothing() {
    assert!(no_tainted_sinks(&analyze_default_off(
        "security-secret-pii-log"
    )));
}

// ── issue #897 catalogue-only sinks (batch 2) ───────────────────────────────

#[test]
fn issue_897_catalogue_sinks_fire() {
    let results = analyze_with_security_sink("security-catalogue-sinks-897");
    assert_candidate(
        &results,
        "src/insecure-randomness.ts",
        "insecure-randomness",
        338,
    );
    assert_candidate(
        &results,
        "src/deprecated-cipher.ts",
        "deprecated-cipher",
        327,
    );
    assert_candidate(
        &results,
        "src/template-escape.ts",
        "template-escape-bypass",
        79,
    );
    assert_candidate(&results, "src/xpath-injection.ts", "xpath-injection", 643);
    assert_candidate(
        &results,
        "src/unsafe-buffer.ts",
        "unsafe-buffer-alloc",
        1188,
    );
    assert_candidate(&results, "src/webview.tsx", "webview-injection", 94);
    assert_candidate(&results, "src/raw-sql-sequelize.ts", "sql-injection", 89);
}

#[test]
fn issue_897_literals_do_not_fire() {
    let results = analyze_with_security_sink("security-catalogue-sinks-897");
    assert!(
        !anchored_on(&results, "src/safe.ts"),
        "literal sink arguments must not be flagged"
    );
}

#[test]
fn issue_897_deferred_and_excluded_patterns_do_not_fire() {
    let results = analyze_with_security_sink("security-catalogue-sinks-897");
    assert!(
        !anchored_on(&results, "src/deferred-not-covered.ts"),
        "deferred client-storage / info-exposure and the excluded libxml-find pattern must not fire"
    );
}

#[test]
fn issue_897_default_off_emits_nothing() {
    assert!(no_tainted_sinks(&analyze_default_off(
        "security-catalogue-sinks-897"
    )));
}

#[test]
fn issue_875_literal_aware_sinks_fire() {
    let results = analyze_with_security_sink("security-literal-sinks-875");
    assert_candidate(
        &results,
        "src/post-message.ts",
        "postmessage-wildcard-origin",
        346,
    );
    assert_candidate(&results, "src/cors.ts", "permissive-cors", 942);
    assert_candidate(&results, "src/cors-satisfies.ts", "permissive-cors", 942);
    assert_candidate(&results, "src/cookie.ts", "insecure-cookie", 614);
    assert_candidate(&results, "src/weak-crypto.ts", "weak-crypto", 327);
    assert_candidate(&results, "src/weak-crypto-as-const.ts", "weak-crypto", 327);
    assert_candidate(&results, "src/weak-crypto-named.ts", "weak-crypto", 327);
    assert_candidate(&results, "src/string-code.ts", "code-injection", 95);
    assert_candidate(
        &results,
        "src/function-constructor.ts",
        "code-injection",
        95,
    );
    assert_candidate(&results, "src/jwt.ts", "jwt-alg-none", 347);
    assert_candidate(
        &results,
        "src/jwt-verify.ts",
        "jwt-verify-missing-algorithms",
        347,
    );
    assert_eq!(
        anchored_count(&results, "src/jwt-verify.ts"),
        3,
        "jwt verify fixture should cover missing options, missing algorithms, and static-wrapped missing algorithms"
    );
    assert_candidate(&results, "src/math-random.ts", "insecure-randomness", 338);
    assert_candidate(&results, "src/metadata-url.ts", "ssrf", 918);
}

#[test]
fn issue_875_literal_safe_and_unprovenanced_forms_do_not_fire() {
    let results = analyze_with_security_sink("security-literal-sinks-875");
    assert!(
        !anchored_on(&results, "src/safe.ts"),
        "safe literal forms and UI randomness must not be flagged"
    );
    assert!(
        !anchored_on(&results, "src/no-provenance.ts"),
        "same-named local cors and jwt helpers must not satisfy provenance-gated rows"
    );
}

#[test]
fn issue_875_default_off_emits_nothing() {
    assert!(no_tainted_sinks(&analyze_default_off(
        "security-literal-sinks-875"
    )));
}

#[test]
fn issue_901_literal_tier_rows_fire() {
    let results = analyze_with_security_sink("security-literal-sinks-901");
    assert_candidate(&results, "src/weak-ecb.ts", "weak-crypto", 327);
    assert_eq!(
        anchored_count(&results, "src/weak-ecb.ts"),
        2,
        "weak ECB fixture should cover namespace and named crypto imports"
    );
    assert_candidate(&results, "src/cleartext.ts", "cleartext-transport", 319);
    assert_eq!(
        anchored_count(&results, "src/cleartext.ts"),
        3,
        "cleartext fixture should cover fetch, axios, and WebSocket literal URLs"
    );
}

#[test]
fn issue_901_safe_literals_do_not_fire() {
    let results = analyze_with_security_sink("security-literal-sinks-901");
    assert!(
        !anchored_on(&results, "src/safe.ts"),
        "encrypted transports and authenticated cipher modes must not be flagged"
    );
}

#[test]
fn issue_901_default_off_emits_nothing() {
    assert!(no_tainted_sinks(&analyze_default_off(
        "security-literal-sinks-901"
    )));
}

#[test]
fn issue_895_tls_validation_disabled_forms_fire() {
    let results = analyze_with_security_sink("security-tls-validation-disabled-895");
    assert_candidate(
        &results,
        "src/https-request.ts",
        "tls-validation-disabled",
        295,
    );
    assert_candidate(
        &results,
        "src/https-agent.ts",
        "tls-validation-disabled",
        295,
    );
    assert_candidate(
        &results,
        "src/tls-connect.ts",
        "tls-validation-disabled",
        295,
    );
    assert_candidate(
        &results,
        "src/named-imports.ts",
        "tls-validation-disabled",
        295,
    );
    assert_eq!(
        anchored_count(&results, "src/named-imports.ts"),
        4,
        "named-import fixture should cover named request, named get, namespace get, and named connect"
    );
    assert_candidate(&results, "src/env.ts", "tls-validation-disabled", 295);
}

#[test]
fn issue_895_safe_and_unprovenanced_forms_do_not_fire() {
    let results = analyze_with_security_sink("security-tls-validation-disabled-895");
    assert!(
        !anchored_on(&results, "src/safe.ts"),
        "TLS validation kept enabled must not be flagged"
    );
    assert!(
        !anchored_on(&results, "src/no-provenance.ts"),
        "same-named local TLS helpers without Node provenance must not fire"
    );
}

#[test]
fn issue_895_default_off_emits_nothing() {
    assert!(no_tainted_sinks(&analyze_default_off(
        "security-tls-validation-disabled-895"
    )));
}
