//! SARIF rendering of security candidates.

use super::*;

/// The SARIF ruleId for a finding. The secret-leak `client-server-leak` keeps its
/// bespoke id; the server-only variant gets `security/server-only-import` so the
/// GitHub Security tab tells "reaches server-only code" apart from "reads a
/// secret"; each `TaintedSink` category gets `security/<category>` so candidates
/// group and label per CWE class.
fn sarif_rule_id(finding: &SecurityFinding) -> String {
    canonical_security_rule_id(finding)
}

fn security_help_text(title: &str) -> String {
    format!(
        "Verify this unverified {title} candidate before acting. Review the source, sink, \
         SARIF code flow, and any runtime or dead-code context. fallow does not prove \
         exploitability, attacker control, or missing sanitization."
    )
}

fn security_help_markdown(title: &str) -> String {
    format!(
        "Verify this unverified **{title}** candidate before acting.\n\n\
         1. Review the source and sink in the SARIF code flow.\n\
         2. Confirm whether attacker-controlled data can reach the sink unsanitized.\n\
         3. Use runtime and dead-code context only as triage signals."
    )
}

fn cwe_taxon_id(cwe: u32) -> String {
    format!("CWE-{cwe}")
}

fn cwe_taxon(cwe: u32) -> serde_json::Value {
    let id = cwe_taxon_id(cwe);
    serde_json::json!({
        "id": id,
        "name": id,
        "shortDescription": { "text": format!("Common Weakness Enumeration {id}") },
        "fullDescription": { "text": format!("MITRE Common Weakness Enumeration {id}") },
        "helpUri": format!("https://cwe.mitre.org/data/definitions/{cwe}.html")
    })
}

fn cwe_relationship(cwe: u32, taxon_index: usize) -> serde_json::Value {
    serde_json::json!({
        "target": {
            "id": cwe_taxon_id(cwe),
            "index": taxon_index,
            "toolComponent": {
                "name": "CWE",
                "index": 0
            }
        },
        "kinds": ["superset"]
    })
}

fn collect_cwes(findings: &[SecurityFinding]) -> Vec<u32> {
    let mut cwes: Vec<u32> = findings.iter().filter_map(|finding| finding.cwe).collect();
    cwes.sort_unstable();
    cwes.dedup();
    cwes
}

fn cwe_index(cwes: &[u32], cwe: u32) -> Option<usize> {
    cwes.iter().position(|existing| *existing == cwe)
}

fn cwe_taxonomy(cwes: &[u32]) -> Option<serde_json::Value> {
    if cwes.is_empty() {
        return None;
    }
    let taxa = cwes.iter().map(|cwe| cwe_taxon(*cwe)).collect::<Vec<_>>();
    Some(serde_json::json!({
        "name": "CWE",
        "fullName": "Common Weakness Enumeration",
        "organization": "MITRE",
        "informationUri": "https://cwe.mitre.org/",
        "taxa": taxa
    }))
}

/// Build the SARIF rule definition for a ruleId, deriving per-category metadata
/// (catalogue title + CWE tag and relationship) for `TaintedSink` findings so
/// CWE grouping survives in SARIF-aware consumers.
fn sarif_rule_def(
    rule_id: &str,
    finding: &SecurityFinding,
    cwe_taxon_index: Option<usize>,
) -> serde_json::Value {
    match finding.kind {
        SecurityFindingKind::ClientServerLeak if is_server_only_leak(finding) => {
            sarif_rule_def_server_only_leak(rule_id)
        }
        SecurityFindingKind::ClientServerLeak => sarif_rule_def_secret_leak(rule_id),
        SecurityFindingKind::TaintedSink => {
            sarif_rule_def_tainted_sink(rule_id, finding, cwe_taxon_index)
        }
    }
}

/// SARIF rule definition for the server-only-import flavor of `ClientServerLeak`.
fn sarif_rule_def_server_only_leak(rule_id: &str) -> serde_json::Value {
    let title = "Client imports server-only code";
    serde_json::json!({
        "id": rule_id,
        "name": title,
        "shortDescription": { "text": "Client imports server-only code candidate (unverified)" },
        "fullDescription": { "text":
            "Unverified candidate, requires verification: a \"use client\" file \
             transitively imports a server-only module (one importing server-only \
             code such as server-only, next/headers, \
             next/server, or node:fs / node:child_process). fallow does not prove this \
             code runs on the client; a module pulled in only through \
             next/dynamic(..., { ssr: false }) is a false positive. fallow excludes a \
             \"use server\" module whose value exports are all async functions, because \
             the bundler replaces its import with action references. A \"use server\" \
             sink has a non-action export (a top-level const, a re-export, or a default \
             value) that carries the server-only import into the client bundle." },
        "help": {
            "text": security_help_text(title),
            "markdown": security_help_markdown(title)
        },
        "helpUri": "https://github.com/fallow-rs/fallow",
        "defaultConfiguration": { "level": "note" }
    })
}

/// SARIF rule definition for the secret-leak flavor of `ClientServerLeak`.
fn sarif_rule_def_secret_leak(rule_id: &str) -> serde_json::Value {
    let title = "Client-server secret leak";
    serde_json::json!({
        "id": rule_id,
        "name": title,
        "shortDescription": { "text": "Client-server secret leak candidate (unverified)" },
        "fullDescription": { "text":
            "Unverified candidate, requires verification: a \"use client\" file \
             transitively imports a module that reads a non-public process.env \
             secret. fallow does not prove the secret reaches client-bundled code." },
        "help": {
            "text": security_help_text(title),
            "markdown": security_help_markdown(title)
        },
        "helpUri": "https://github.com/fallow-rs/fallow",
        "defaultConfiguration": { "level": "note" }
    })
}

/// SARIF rule definition for `TaintedSink` findings, attaching CWE tags and the
/// CWE taxonomy relationship when the finding carries a CWE id.
fn sarif_rule_def_tainted_sink(
    rule_id: &str,
    finding: &SecurityFinding,
    cwe_taxon_index: Option<usize>,
) -> serde_json::Value {
    let title = finding
        .category
        .as_deref()
        .and_then(security_catalogue_title)
        .or(finding.category.as_deref())
        .unwrap_or("tainted-sink");
    let mut rule = serde_json::json!({
        "id": rule_id,
        "name": title,
        "shortDescription": { "text": format!("{title} candidate (unverified)") },
        "fullDescription": { "text": format!(
            "Unverified candidate, requires verification: {title}. fallow flags a \
             syntactic sink reached by a non-literal argument; it does not prove the \
             value is attacker-controlled or reaches the sink unsanitized."
        ) },
        "help": {
            "text": security_help_text(title),
            "markdown": security_help_markdown(title)
        },
        "helpUri": "https://github.com/fallow-rs/fallow",
        "defaultConfiguration": { "level": "note" }
    });
    if let Some(cwe) = finding.cwe {
        rule["properties"] = serde_json::json!({
            "tags": [format!("external/cwe/cwe-{cwe}")]
        });
        if let Some(taxon_index) = cwe_taxon_index {
            rule["relationships"] = serde_json::json!([cwe_relationship(cwe, taxon_index)]);
        }
    }
    rule
}

fn hop_role_token(role: TraceHopRole) -> &'static str {
    match role {
        TraceHopRole::ClientBoundary => "client-boundary",
        TraceHopRole::UntrustedSource => "untrusted-source",
        TraceHopRole::ModuleSource => "module-source",
        TraceHopRole::Intermediate => "intermediate",
        TraceHopRole::SecretSource => "secret-source",
        TraceHopRole::Sink => "sink",
    }
}

fn sarif_thread_flow_location(hop: &TraceHop, sources: &mut SourceLines) -> serde_json::Value {
    let role = hop_role_token(hop.role);
    serde_json::json!({
        "location": sarif_location(&hop.path, hop.line, hop.col, sources),
        "kinds": [role],
        "properties": { "fallowTraceRole": role }
    })
}

fn primary_code_flow_hops(finding: &SecurityFinding) -> &[TraceHop] {
    if let Some(reachability) = finding.reachability.as_ref()
        && !reachability.untrusted_source_trace.is_empty()
    {
        return &reachability.untrusted_source_trace;
    }
    &finding.trace
}

fn sarif_code_flows(
    finding: &SecurityFinding,
    sources: &mut SourceLines,
) -> Option<serde_json::Value> {
    let hops = primary_code_flow_hops(finding);
    if hops.is_empty() {
        return None;
    }
    let locations = hops
        .iter()
        .map(|hop| sarif_thread_flow_location(hop, sources))
        .collect::<Vec<_>>();
    Some(serde_json::json!([
        {
            "threadFlows": [
                { "locations": locations }
            ]
        }
    ]))
}

fn push_related_location(
    related: &mut Vec<serde_json::Value>,
    hop: &TraceHop,
    sources: &mut SourceLines,
) {
    let location = sarif_location(&hop.path, hop.line, hop.col, sources);
    if !related.iter().any(|existing| existing == &location) {
        related.push(location);
    }
}

fn sarif_related_locations(
    finding: &SecurityFinding,
    sources: &mut SourceLines,
) -> Vec<serde_json::Value> {
    let mut related = Vec::new();
    for hop in &finding.trace {
        push_related_location(&mut related, hop, sources);
    }
    if let Some(reachability) = finding.reachability.as_ref() {
        for hop in &reachability.untrusted_source_trace {
            push_related_location(&mut related, hop, sources);
        }
    }
    related
}

const fn sarif_level(severity: SecuritySeverity) -> &'static str {
    match severity {
        SecuritySeverity::High | SecuritySeverity::Medium => "warning",
        SecuritySeverity::Low => "note",
    }
}

/// Build the SARIF `result` object for a single finding, composing the
/// candidate-framed message, related locations, fingerprint, and code flows.
fn sarif_result_for_finding(
    finding: &SecurityFinding,
    sources: &mut SourceLines,
) -> serde_json::Value {
    let rule_id = sarif_rule_id(finding);
    let mut message = dead_code_hint(finding).map_or_else(
        || finding.evidence.clone(),
        |hint| format!("{} Dead-code cross-link: {hint}.", finding.evidence),
    );
    if let Some(hint) = source_reachability_hint(finding) {
        message.push(' ');
        message.push_str(hint);
    }
    if let Some(runtime) = finding.runtime.as_ref() {
        message.push_str(" Runtime context: ");
        message.push_str(&runtime_hint_text(runtime));
        message.push('.');
    }
    let related = sarif_related_locations(finding, sources);
    // Stable dedup key for GHAS: rule + anchor path + line + column. Without
    // partialFingerprints, every run re-opens previously triaged alerts.
    // It is the `finding_id` that the shared pipeline set, the same value as
    // the JSON field, so the two never drift (issue #900).
    let mut result = serde_json::json!({
        "ruleId": rule_id,
        "level": sarif_level(finding.severity),
        "message": { "text": message },
        "locations": [sarif_location(&finding.path, finding.line, finding.col, sources)],
        "relatedLocations": related,
        "partialFingerprints": { "fallowSecurity/v2": finding.finding_id },
    });
    if let Some(code_flows) = sarif_code_flows(finding, sources) {
        result["codeFlows"] = code_flows;
    }
    result
}

/// Collect one SARIF rule definition per distinct ruleId present in `findings`,
/// in first-seen order, attaching the CWE taxonomy index when available.
fn sarif_rule_defs(findings: &[SecurityFinding], cwes: &[u32]) -> Vec<serde_json::Value> {
    let mut seen: Vec<String> = Vec::new();
    let mut rules: Vec<serde_json::Value> = Vec::new();
    for finding in findings {
        let rule_id = sarif_rule_id(finding);
        if seen.iter().any(|s| s == &rule_id) {
            continue;
        }
        seen.push(rule_id.clone());
        let cwe_taxon_index = finding.cwe.and_then(|cwe| cwe_index(cwes, cwe));
        rules.push(sarif_rule_def(&rule_id, finding, cwe_taxon_index));
    }
    rules
}

/// SARIF output. Maps the candidate's verification-priority tier to SARIF
/// `level` while keeping the message text candidate-framed. Each finding's ruleId is
/// per-category (`security/<category>` for tainted-sink, `security/client-server-leak`
/// for the graph rule); the `rules` array carries one definition per distinct
/// ruleId present, with the CWE tag for tainted-sink categories. Detector trace
/// hops and source-reachability hops become `relatedLocations` of the result.
#[must_use]
pub(super) fn render_sarif(output: &SecurityOutput, root: &Path) -> String {
    let gate = output
        .gate
        .as_ref()
        .and_then(|gate| serde_json::to_value(gate).ok());
    let sarif = build_security_sarif(&output.security_findings, gate.as_ref(), root);
    serde_json::to_string_pretty(&sarif)
        .unwrap_or_else(|_| "{\"error\":\"failed to serialize sarif\"}".to_owned())
}

/// Build the native security SARIF document from typed findings.
///
/// Both the direct security command and `fallow report --from` use this
/// function so CWE taxonomy, traces, fingerprints, and gate metadata cannot
/// drift between analyze-once and direct rendering.
///
/// Finding paths are relative to `root`. The builder reads the source lines
/// under `root` to give each `startColumn` in UTF-16 code units.
#[must_use]
pub fn build_security_sarif(
    security_findings: &[SecurityFinding],
    gate: Option<&serde_json::Value>,
    root: &Path,
) -> serde_json::Value {
    let cwes = collect_cwes(security_findings);
    let mut sources = SourceLines::with_root(root);
    let results: Vec<serde_json::Value> = security_findings
        .iter()
        .map(|finding| sarif_result_for_finding(finding, &mut sources))
        .collect();
    let rules = sarif_rule_defs(security_findings, &cwes);

    let mut run = serde_json::json!({
        "tool": { "driver": {
            "name": "fallow",
            "version": env!("CARGO_PKG_VERSION"),
            "informationUri": "https://github.com/fallow-rs/fallow",
            "rules": rules,
        }},
        "results": results,
    });
    if let Some(taxonomy) = cwe_taxonomy(&cwes) {
        run["taxonomies"] = serde_json::json!([taxonomy]);
        run["tool"]["driver"]["supportedTaxonomies"] = serde_json::json!([
            { "name": "CWE", "index": 0 }
        ]);
    }
    // Gate verdict rides as a RUN-level property, never on result severity.
    // Result levels come from candidate review-priority severity and deliberately
    // avoid `error`, so GHAS does not frame candidates as confirmed problems.
    if let Some(gate) = gate {
        run["properties"] = serde_json::json!({ "fallowGate": gate });
    }

    serde_json::json!({
        "version": "2.1.0",
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "runs": [run],
    })
}

pub(super) type SourceLines = fallow_output::SarifSourceSnippetCache;

pub(super) fn sarif_location(
    path: &Path,
    line: u32,
    col: u32,
    sources: &mut SourceLines,
) -> serde_json::Value {
    let column = sources.start_column(path, line, col);
    serde_json::json!({
        "physicalLocation": {
            "artifactLocation": { "uri": path.to_string_lossy().replace('\\', "/") },
            "region": { "startLine": line.max(1), "startColumn": column }
        }
    })
}
