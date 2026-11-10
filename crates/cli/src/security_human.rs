//! Human rendering of `fallow security` output.

use super::*;

/// One-line gate verdict header. Leads with the ACTION ("REVIEW REQUIRED") and
/// immediately qualifies with the candidate framing, so a human never reads the
/// gate as "fallow confirmed a vulnerability". The wire `verdict` token stays
/// `fail`; only this human prose says "REVIEW REQUIRED".
pub(super) fn gate_human_header(gate: &SecurityGate) -> String {
    use crate::report::plural;
    let checked = match gate.mode {
        SecurityGateMode::New => "in changed lines",
        SecurityGateMode::NewlyReachable => "newly reachable from entry points",
    };
    match gate.verdict {
        SecurityGateVerdict::Fail => format!(
            "Gate: REVIEW REQUIRED, {} new security item{} {checked}. fallow has not confirmed a vulnerability.",
            gate.new_count,
            plural(gate.new_count),
        ),
        SecurityGateVerdict::Pass => {
            format!("Gate: PASS, no new security items {checked}.")
        }
    }
}

fn unresolved_callee_human_hint(output: &SecurityOutput) -> Option<String> {
    let diagnostics = output.unresolved_callee_diagnostics.as_ref()?;
    let top_reason = diagnostics.by_reason.first()?;
    let top_file = diagnostics.top_files.first()?;
    Some(format!(
        "Most unresolved callees: {} in {}.",
        unresolved_callee_reason_label(top_reason.reason),
        top_file.path
    ))
}

pub(super) fn unresolved_callee_reason_label(
    reason: fallow_types::extract::SkippedSecurityCalleeReason,
) -> &'static str {
    match reason {
        fallow_types::extract::SkippedSecurityCalleeReason::ComputedMember => "computed-member",
        fallow_types::extract::SkippedSecurityCalleeReason::DynamicDispatch => "dynamic-dispatch",
        fallow_types::extract::SkippedSecurityCalleeReason::UnsupportedAssignmentObject => {
            "unsupported-assignment-object"
        }
    }
}

#[must_use]
pub(super) fn render_human_summary(output: &SecurityOutput) -> String {
    use crate::report::plural;
    use std::fmt::Write as _;

    let mut out = String::new();
    if let Some(gate) = &output.gate {
        out.push_str(&gate_human_header(gate));
        out.push('\n');
    }
    let count = output.security_findings.len();
    if count == 0 {
        out.push_str("Security review: no items to check in the scanned code.\n");
    } else {
        let _ = writeln!(
            out,
            "Security review: {count} item{} to check. These are unverified security candidates, not confirmed vulnerabilities.",
            plural(count),
        );
        out.push_str(
            "Next: check whether the listed code can run with unsafe input, secrets, or settings, and whether anything blocks the risk.\n",
        );
    }
    if output.unresolved_edge_files > 0 {
        let n = output.unresolved_edge_files;
        let verb = if n == 1 { "uses" } else { "use" };
        let _ = writeln!(
            out,
            "Blind spot: {n} client file{} {verb} dynamic imports that fallow could not follow.",
            plural(n)
        );
    }
    if output.unresolved_callee_sites > 0 {
        let n = output.unresolved_callee_sites;
        let verb = if n == 1 { "uses" } else { "use" };
        let _ = writeln!(
            out,
            "Blind spot: {n} call site{} {verb} code patterns that fallow could not resolve.",
            plural(n)
        );
        if let Some(hint) = unresolved_callee_human_hint(output) {
            let _ = writeln!(out, "{hint}");
        }
    }
    if has_no_client_roots(output) {
        let _ = writeln!(out, "Blind spot: {NO_CLIENT_ROOTS_LINE}");
    }
    out
}

/// The human line for `client-server-leak-no-client-roots`, shared by the
/// full and the summary renderer.
const NO_CLIENT_ROOTS_LINE: &str = "no file has a top-level \"use client\" directive, so the \
     client-server-leak rule had no starting point. Its zero count does not mean that no secret \
     reaches client code.";

/// Whether the run recorded that `client-server-leak` had no `"use client"`
/// file to start from.
fn has_no_client_roots(output: &SecurityOutput) -> bool {
    output.workspace_diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.kind,
            fallow_config::WorkspaceDiagnosticKind::ClientServerLeakNoClientRoots
        )
    })
}

/// Human output. Frames findings as candidates and states the next human action
/// per finding; surfaces the unresolved-edge blind spot as a counted line.
#[must_use]
#[expect(
    clippy::format_push_string,
    reason = "small report renderer; readability over avoiding the extra allocation"
)]
pub(super) fn render_human(output: &SecurityOutput) -> String {
    use crate::report::plural;

    let mut out = String::new();
    push_human_gate(&mut out, output);
    let count = output.security_findings.len();
    out.push_str(&format!("Security review: {count} item{}", plural(count)));
    if count == 0 {
        out.push_str(" to check in the scanned code.\n");
    } else {
        out.push_str(" to check.\n");
        out.push_str(
            "These are unverified security candidates, not confirmed vulnerabilities. Check whether the listed code can run with unsafe input, secrets, or settings, and whether anything blocks the risk.\n",
        );
    }
    out.push('\n');

    if output.security_findings.is_empty() {
        out.push_str("No security details to show.\n");
    } else {
        push_human_findings(&mut out, &output.security_findings);
    }

    push_human_blind_spots(&mut out, output);

    out.push_str(&format!(
        "\nResult: {count} security item{} to check.",
        plural(count),
    ));
    if count > 0 {
        out.push_str(" Review the listed evidence and trace before changing code.");
    }
    out.push('\n');
    out
}

fn push_human_gate(out: &mut String, output: &SecurityOutput) {
    if let Some(gate) = &output.gate {
        out.push_str(&gate_human_header(gate));
        out.push_str("\n\n");
    }
}

fn push_human_findings(out: &mut String, findings: &[SecurityFinding]) {
    for finding in findings {
        push_human_finding(out, finding);
    }
}

fn push_human_finding(out: &mut String, finding: &SecurityFinding) {
    use std::fmt::Write as _;

    push_human_finding_header(out, finding);
    let _ = writeln!(out, "    evidence: {}", finding.evidence);
    if let Some(hint) = dead_code_hint(finding) {
        let _ = writeln!(out, "    dead-code: {hint}");
    }
    if let Some(runtime) = finding.runtime.as_ref() {
        let _ = writeln!(out, "    runtime: {}", runtime_hint_text(runtime));
    }
    push_human_reachability(out, finding);
    push_human_import_trace(out, finding);
    push_human_next_step(out, finding);
    out.push('\n');
}

fn push_human_finding_header(out: &mut String, finding: &SecurityFinding) {
    use colored::Colorize;
    use std::fmt::Write as _;

    let kind = security_finding_label(finding);
    let (glyph, label) = human_severity_marker(finding.severity);
    let _ = writeln!(
        out,
        "{} {label} {kind}  {}:{}",
        glyph,
        finding.path.to_string_lossy().replace('\\', "/").bold(),
        finding.line,
    );
}

fn push_human_reachability(out: &mut String, finding: &SecurityFinding) {
    use std::fmt::Write as _;

    let Some(reach) = finding.reachability.as_ref() else {
        return;
    };
    let entry = if reach.reachable_from_entry {
        "reachable from a runtime entry point"
    } else {
        "not reached from any runtime entry point"
    };
    let boundary = if reach.crosses_boundary {
        "; crosses an architecture boundary"
    } else {
        ""
    };
    let _ = writeln!(
        out,
        "    code path: {entry} (blast radius {}){boundary}",
        reach.blast_radius,
    );
    if reach.reachable_from_untrusted_source {
        push_human_untrusted_trace(out, finding);
    }
}

fn push_human_untrusted_trace(out: &mut String, finding: &SecurityFinding) {
    use std::fmt::Write as _;

    let Some(reach) = finding.reachability.as_ref() else {
        return;
    };
    let hops = reach.untrusted_source_hop_count.unwrap_or(0);
    let _ = writeln!(
        out,
        "    input path: this module is reachable from a module that receives \
         untrusted input via {hops} import hop{}",
        crate::report::plural(hops as usize),
    );
    if !reach.untrusted_source_trace.is_empty() {
        out.push_str("    input import trace:\n");
        for hop in &reach.untrusted_source_trace {
            let _ = writeln!(
                out,
                "      {}:{} ({})",
                hop.path.to_string_lossy().replace('\\', "/"),
                hop.line,
                hop_role_label(hop.role),
            );
        }
    }
}

fn push_human_import_trace(out: &mut String, finding: &SecurityFinding) {
    use std::fmt::Write as _;

    if finding.trace.is_empty() {
        return;
    }
    out.push_str("    import trace:\n");
    for hop in &finding.trace {
        let _ = writeln!(
            out,
            "      {}:{} ({})",
            hop.path.to_string_lossy().replace('\\', "/"),
            hop.line,
            hop_role_label(hop.role),
        );
    }
}

fn push_human_next_step(out: &mut String, finding: &SecurityFinding) {
    if is_server_only_leak(finding) {
        out.push_str(
            "    Next: check whether this server-only code is meant to run on the client. \
             If it is pulled in only through next/dynamic(..., { ssr: false }), type-only, \
             or removed at build time, mark it as a false positive. fallow already \
             excludes a \"use server\" module whose value exports are all async \
             functions. If the sink is a \"use server\" module, move its non-action \
             export (a top-level const, a re-export, or a default value) to a \
             server-only file.\n",
        );
    } else if matches!(finding.kind, SecurityFindingKind::ClientServerLeak) {
        out.push_str(
            "    Next: check whether this import can ship a secret to the browser. If \
             it is type-only, server-only, or removed at build time, mark it as a false \
             positive.\n",
        );
    } else if finding.dead_code.is_some() {
        out.push_str(
            "    Next: first verify the dead-code finding. If the code is safe to \
             remove, delete it. Otherwise check and harden the risky call.\n",
        );
    } else {
        out.push_str(
            "    Next: check whether unsafe input, secrets, or settings can reach this \
             risky call without a safe guard. If not, mark it as a false positive.\n",
        );
    }
}

fn push_human_blind_spots(out: &mut String, output: &SecurityOutput) {
    use crate::report::plural;
    use std::fmt::Write as _;

    if output.unresolved_edge_files > 0 {
        let n = output.unresolved_edge_files;
        let verb = if n == 1 { "uses" } else { "use" };
        let _ = writeln!(
            out,
            "{} Blind spot: {n} client file{} {verb} dynamic imports that fallow could not \
             follow. Code behind those imports may be missing from this report.",
            "[I]".blue().bold(),
            plural(n),
        );
    }

    if output.unresolved_callee_sites > 0 {
        let n = output.unresolved_callee_sites;
        let verb = if n == 1 { "uses" } else { "use" };
        let _ = writeln!(
            out,
            "{} Blind spot: {n} call site{} {verb} code patterns that fallow could not resolve, \
             such as dynamic dispatch, computed members, or aliased bindings.",
            "[I]".blue().bold(),
            plural(n),
        );
        if let Some(hint) = unresolved_callee_human_hint(output) {
            let _ = writeln!(out, "    {hint}");
        }
    }

    if has_no_client_roots(output) {
        let _ = writeln!(
            out,
            "{} Blind spot: {NO_CLIENT_ROOTS_LINE}",
            "[I]".blue().bold(),
        );
    }
}

/// Render the human-facing label for a finding. The secret-leak
/// `ClientServerLeak` keeps its bespoke kebab kind; the server-only variant uses
/// its own kebab label so a reader tells the two apart; `TaintedSink` uses the
/// catalogue title plus the CWE number carried on the finding.
fn security_finding_label(finding: &SecurityFinding) -> String {
    match finding.kind {
        SecurityFindingKind::ClientServerLeak if is_server_only_leak(finding) => {
            "server-only-import".to_string()
        }
        SecurityFindingKind::ClientServerLeak => "client-server-leak".to_string(),
        SecurityFindingKind::TaintedSink => {
            let title = finding
                .category
                .as_deref()
                .and_then(security_catalogue_title)
                .or(finding.category.as_deref())
                .unwrap_or("tainted-sink");
            match finding.cwe {
                Some(cwe) => format!("{title} (CWE-{cwe})"),
                None => title.to_string(),
            }
        }
    }
}

fn human_severity_marker(severity: SecuritySeverity) -> (colored::ColoredString, &'static str) {
    use colored::Colorize;
    match severity {
        SecuritySeverity::High => ("[H]".red().bold(), "high"),
        SecuritySeverity::Medium => ("[M]".yellow().bold(), "medium"),
        SecuritySeverity::Low => ("[L]".blue().bold(), "low"),
    }
}

pub(super) fn dead_code_hint(finding: &SecurityFinding) -> Option<String> {
    let context = finding.dead_code.as_ref()?;
    match context.kind {
        SecurityDeadCodeKind::UnusedFile => Some(
            "also reported as unused-file; delete this file instead of hardening the sink"
                .to_string(),
        ),
        SecurityDeadCodeKind::UnusedExport => Some(format!(
            "also reported as unused-export{}; remove the export instead of hardening the sink",
            context
                .export_name
                .as_ref()
                .map_or(String::new(), |name| format!(" `{name}`"))
        )),
    }
}

pub(super) const fn hop_role_label(role: TraceHopRole) -> &'static str {
    match role {
        TraceHopRole::ClientBoundary => "client boundary",
        TraceHopRole::UntrustedSource => "untrusted source",
        TraceHopRole::ModuleSource => "source module",
        TraceHopRole::Intermediate => "intermediate",
        TraceHopRole::SecretSource => "secret source",
        TraceHopRole::Sink => "sink site",
    }
}

pub(super) fn source_reachability_hint(finding: &SecurityFinding) -> Option<&'static str> {
    finding
        .reachability
        .as_ref()
        .filter(|reach| reach.reachable_from_untrusted_source)
        .map(|_| {
            "Module-level context: the sink module is reachable from an untrusted-source module; fallow does not prove value flow."
        })
}

pub(super) fn runtime_hint_text(runtime: &SecurityRuntimeContext) -> String {
    use std::fmt::Write as _;

    let mut text = format!(
        "{} in {}:{}",
        runtime_state_label(runtime.state),
        runtime.function,
        runtime.line
    );
    if let Some(invocations) = runtime.invocations {
        let _ = write!(
            text,
            " ({} invocation{})",
            invocations,
            crate::report::plural(invocations as usize)
        );
    }
    if let Some(evidence) = runtime.evidence.as_deref() {
        text.push_str("; ");
        text.push_str(evidence);
    }
    text
}

const fn runtime_state_label(state: SecurityRuntimeState) -> &'static str {
    match state {
        SecurityRuntimeState::RuntimeHot => "runtime-hot",
        SecurityRuntimeState::RuntimeCold => "runtime-cold",
        SecurityRuntimeState::NeverExecuted => "never-executed",
        SecurityRuntimeState::LowTraffic => "low-traffic",
        SecurityRuntimeState::CoverageUnavailable => "coverage-unavailable",
        SecurityRuntimeState::RuntimeUnknown => "runtime-unknown",
    }
}

/// The `category` string distinguishing the server-only-import sink from the
/// secret-leak sink (both `ClientServerLeak` kind). Matches the constant in
/// `crates/core/src/analyze/security/mod.rs`.
const SERVER_ONLY_CATEGORY: &str = "server-only-import";

/// Whether a `ClientServerLeak` finding is the server-only-import variant rather
/// than the original secret-leak variant. Keys on `category` because both share
/// the `ClientServerLeak` kind and the same rule.
pub(super) fn is_server_only_leak(finding: &SecurityFinding) -> bool {
    matches!(finding.kind, SecurityFindingKind::ClientServerLeak)
        && finding.category.as_deref() == Some(SERVER_ONLY_CATEGORY)
}
