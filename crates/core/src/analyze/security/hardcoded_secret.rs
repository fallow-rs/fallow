//! Opt-in hardcoded-secret candidate detector.
//!
//! This detector consumes extractor-captured static string literals and emits
//! conservative first-party candidates only when a provider token shape matches,
//! or high entropy is corroborated by a secret-shaped identifier.

use rustc_hash::FxHashMap;

use fallow_types::extract::{ModuleInfo, SinkLiteralValue, SinkShape, SinkSite};
use fallow_types::results::{
    SecurityCandidate, SecurityCandidateBoundary, SecurityCandidateSink, SecurityFinding,
    SecurityFindingKind, SecuritySeverity, TraceHop, TraceHopRole,
};
use fallow_types::suppress::IssueKind;

use super::tainted_sink::{CategoryFilter, build_actions, is_low_value_anchor};
use super::{LineOffsetsMap, byte_offset_to_line_col};
use crate::discover::FileId;
use crate::graph::{ModuleGraph, ModuleNode};
use crate::suppress::SuppressionContext;

pub const CATEGORY_ID: &str = "hardcoded-secret";
pub const CATEGORY_TITLE: &str = "Hardcoded secret candidate";
const CWE_ID: u32 = 798;
const MIN_ENTROPY_LENGTH: usize = 20;
const MIN_ENTROPY_BITS_PER_CHAR: f64 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SecretSignal {
    Provider(&'static str),
    EntropyWithIdentifier,
}

#[must_use]
pub fn find_hardcoded_secret_candidates(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    category_filter: &CategoryFilter,
    root: &std::path::Path,
) -> Vec<SecurityFinding> {
    if !category_filter.explicitly_admits(CATEGORY_ID) {
        return Vec::new();
    }

    let modules_by_id: FxHashMap<FileId, &ModuleInfo> =
        modules.iter().map(|m| (m.file_id, m)).collect();
    let ctx = HardcodedSecretContext {
        suppressions,
        line_offsets_by_file,
        root,
    };

    let mut findings = Vec::new();
    for node in &graph.modules {
        let Some(module) = modules_by_id.get(&node.file_id) else {
            continue;
        };
        collect_module_secret_candidates(&mut findings, node, module, &ctx);
    }

    sort_security_findings(&mut findings);
    findings
}

struct HardcodedSecretContext<'a> {
    suppressions: &'a SuppressionContext<'a>,
    line_offsets_by_file: &'a LineOffsetsMap<'a>,
    root: &'a std::path::Path,
}

fn collect_module_secret_candidates(
    findings: &mut Vec<SecurityFinding>,
    node: &ModuleNode,
    module: &ModuleInfo,
    ctx: &HardcodedSecretContext<'_>,
) {
    if module.security_sinks.is_empty() {
        return;
    }
    let rel_path = node.path.strip_prefix(ctx.root).unwrap_or(&node.path);
    if is_low_value_anchor(rel_path) {
        return;
    }
    let file_id = node.file_id;
    if ctx
        .suppressions
        .is_file_suppressed(file_id, IssueKind::SecuritySink)
    {
        return;
    }

    for sink in &module.security_sinks {
        if let Some(finding) = build_hardcoded_secret_finding(node, file_id, sink, ctx) {
            findings.push(finding);
        }
    }
}

fn build_hardcoded_secret_finding(
    node: &ModuleNode,
    file_id: FileId,
    sink: &SinkSite,
    ctx: &HardcodedSecretContext<'_>,
) -> Option<SecurityFinding> {
    if sink.sink_shape != SinkShape::SecretLiteral {
        return None;
    }
    let Some(SinkLiteralValue::String(value)) = sink.arg_literal.as_ref() else {
        return None;
    };
    let signal = classify_secret_literal(&sink.callee_path, value)?;
    let (line, col) = byte_offset_to_line_col(ctx.line_offsets_by_file, file_id, sink.span_start);
    if ctx
        .suppressions
        .is_suppressed(file_id, line, IssueKind::SecuritySink)
    {
        return None;
    }

    Some(create_hardcoded_secret_finding(
        node, sink, signal, line, col,
    ))
}

fn create_hardcoded_secret_finding(
    node: &ModuleNode,
    sink: &SinkSite,
    signal: SecretSignal,
    line: u32,
    col: u32,
) -> SecurityFinding {
    let evidence = redacted_evidence(signal, &sink.callee_path);
    let candidate = SecurityCandidate {
        source_kind: None,
        sink: SecurityCandidateSink {
            path: node.path.clone(),
            line,
            col,
            category: Some(CATEGORY_ID.to_string()),
            cwe: Some(CWE_ID),
            callee: Some(sink.callee_path.clone()),
            url_shape: None,
        },
        boundary: SecurityCandidateBoundary::default(),
        network: None,
    };
    let path = node.path.clone();

    SecurityFinding {
        finding_id: String::new(),
        kind: SecurityFindingKind::TaintedSink,
        category: Some(CATEGORY_ID.to_string()),
        cwe: Some(CWE_ID),
        path: path.clone(),
        line,
        col,
        evidence,
        source_backed: false,
        source_read: None,
        severity: SecuritySeverity::Low,
        trace: vec![TraceHop {
            path,
            line,
            col,
            role: TraceHopRole::Sink,
        }],
        actions: build_actions(),
        reachability: None,
        dead_code: None,
        candidate,
        taint_flow: None,
        runtime: None,
        attack_surface: None,
    }
}

fn sort_security_findings(findings: &mut [SecurityFinding]) {
    findings.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.line.cmp(&b.line))
            .then(a.col.cmp(&b.col))
            .then(a.category.cmp(&b.category))
    });
}

fn classify_secret_literal(context_name: &str, value: &str) -> Option<SecretSignal> {
    if is_allowlisted_literal(value) {
        return None;
    }
    if let Some(provider) = provider_secret_family(value) {
        return Some(SecretSignal::Provider(provider));
    }
    if is_secret_shaped_identifier(context_name) && has_high_entropy(value) {
        return Some(SecretSignal::EntropyWithIdentifier);
    }
    None
}

fn redacted_evidence(signal: SecretSignal, context_name: &str) -> String {
    match signal {
        SecretSignal::Provider(provider) => format!(
            "Static string literal matches the {provider} credential shape. Verify whether this is a real secret, then rotate and move it to secret storage if needed."
        ),
        SecretSignal::EntropyWithIdentifier => format!(
            "High-entropy static string literal assigned to secret-shaped identifier `{context_name}`. Verify whether this is a real secret, then rotate and move it to secret storage if needed."
        ),
    }
}

fn provider_secret_family(value: &str) -> Option<&'static str> {
    if is_aws_access_key(value) {
        return Some("AWS access key");
    }
    if is_github_token(value) {
        return Some("GitHub token");
    }
    if matches_prefixed_len(value, "glpat-", 20) {
        return Some("GitLab token");
    }
    if is_slack_token(value) {
        return Some("Slack token");
    }
    if matches_prefixed_min_len(value, "sk_live_", 16)
        || matches_prefixed_min_len(value, "rk_live_", 16)
    {
        return Some("Stripe key");
    }
    if matches_prefixed_min_len(value, "sk-ant-", 20) {
        return Some("Anthropic key");
    }
    if matches_prefixed_min_len(value, "sk-proj-", 20) {
        return Some("OpenAI project key");
    }
    if matches_prefixed_len(value, "AIza", 35) {
        return Some("Google API key");
    }
    if is_sendgrid_key(value) {
        return Some("SendGrid key");
    }
    if matches_prefixed_len(value, "npm_", 36) {
        return Some("npm token");
    }
    if matches_prefixed_min_len(value, "pypi-AgEI", 60) {
        return Some("PyPI token");
    }
    if matches_prefixed_min_len(value, "EAAA", 20) || matches_prefixed_min_len(value, "sq0atp-", 20)
    {
        return Some("Square token");
    }
    if ["shpat_", "shpss_", "shpca_", "shppa_"]
        .iter()
        .any(|prefix| matches_prefixed_min_len(value, prefix, 20))
    {
        return Some("Shopify token");
    }
    if matches_prefixed_min_len(value, "dp.pt.", 20) {
        return Some("Doppler token");
    }
    if is_digitalocean_token(value) {
        return Some("DigitalOcean token");
    }
    if matches_prefixed_min_len(value, "lin_api_", 20) {
        return Some("Linear token");
    }
    if matches_prefixed_min_len(value, "PMAK-", 20) {
        return Some("Postman key");
    }
    if matches_prefixed_min_len(value, "hf_", 20) {
        return Some("Hugging Face token");
    }
    if matches_prefixed_min_len(value, "dapi", 20) {
        return Some("Databricks token");
    }
    if is_telegram_bot_token(value) {
        return Some("Telegram bot token");
    }
    if matches_prefixed_min_len(value, "AGE-SECRET-KEY-1", 30) {
        return Some("age secret key");
    }
    if is_pem_private_key(value) {
        return Some("PEM private key");
    }
    None
}

fn is_aws_access_key(value: &str) -> bool {
    (value.starts_with("AKIA") || value.starts_with("ASIA"))
        && value.len() == 20
        && value
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit())
}

fn is_github_token(value: &str) -> bool {
    ["ghp_", "gho_", "ghu_", "ghs_", "ghr_"]
        .iter()
        .any(|prefix| matches_prefixed_len(value, prefix, 36))
        || matches_prefixed_min_len(value, "github_pat_", 70)
}

fn is_slack_token(value: &str) -> bool {
    ["xoxb-", "xoxp-", "xoxa-", "xoxr-", "xoxs-"]
        .iter()
        .any(|prefix| matches_prefixed_min_len(value, prefix, 16))
}

fn is_sendgrid_key(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    parts.len() == 3 && parts[0] == "SG" && parts[1].len() >= 10 && parts[2].len() >= 10
}

fn is_digitalocean_token(value: &str) -> bool {
    ["dop_v1_", "dor_v1_", "dot_v1_", "doo_v1_"]
        .iter()
        .any(|prefix| {
            value.strip_prefix(prefix).is_some_and(|tail| {
                tail.len() == 64 && tail.chars().all(|ch| ch.is_ascii_hexdigit())
            })
        })
}

fn is_telegram_bot_token(value: &str) -> bool {
    let Some((id, token)) = value.split_once(':') else {
        return false;
    };
    (8..=10).contains(&id.len())
        && id.chars().all(|ch| ch.is_ascii_digit())
        && token.starts_with("AA")
        && token.len() >= 20
}

fn is_pem_private_key(value: &str) -> bool {
    value.contains("-----BEGIN")
        && value.contains("PRIVATE KEY-----")
        && !value.contains("PUBLIC KEY-----")
}

fn matches_prefixed_len(value: &str, prefix: &str, tail_len: usize) -> bool {
    value
        .strip_prefix(prefix)
        .is_some_and(|tail| tail.len() == tail_len && is_token_tail(tail))
}

fn matches_prefixed_min_len(value: &str, prefix: &str, min_tail_len: usize) -> bool {
    value
        .strip_prefix(prefix)
        .is_some_and(|tail| tail.len() >= min_tail_len && is_token_tail(tail))
}

fn is_token_tail(value: &str) -> bool {
    value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
}

fn is_secret_shaped_identifier(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "apikey",
        "api_key",
        "accesskey",
        "access_key",
        "privatekey",
        "private_key",
        "clientsecret",
        "client_secret",
        "token",
        "secret",
        "password",
        "passwd",
        "credential",
        "jwt",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn has_high_entropy(value: &str) -> bool {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() < MIN_ENTROPY_LENGTH {
        return false;
    }
    let mut counts: FxHashMap<char, usize> = FxHashMap::default();
    for ch in &chars {
        *counts.entry(*ch).or_default() += 1;
    }
    let len = chars.len() as f64;
    let entropy = counts.values().fold(0.0, |acc, count| {
        let p = *count as f64 / len;
        p.mul_add(-p.log2(), acc)
    });
    entropy >= MIN_ENTROPY_BITS_PER_CHAR && has_mixed_secret_charset(value)
}

fn has_mixed_secret_charset(value: &str) -> bool {
    let has_lower = value.chars().any(|ch| ch.is_ascii_lowercase());
    let has_upper = value.chars().any(|ch| ch.is_ascii_uppercase());
    let has_digit = value.chars().any(|ch| ch.is_ascii_digit());
    let has_symbol = value
        .chars()
        .any(|ch| matches!(ch, '_' | '-' | '.' | '/' | '+' | '='));
    [has_lower, has_upper, has_digit, has_symbol]
        .into_iter()
        .filter(|present| *present)
        .count()
        >= 3
}

fn is_allowlisted_literal(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    if lower.contains("example")
        || lower.contains("placeholder")
        || lower.contains("changeme")
        || lower.contains("dummy")
        || lower.contains("fake")
        || lower.starts_with("data:")
        || lower.starts_with("sha256-")
        || lower.starts_with("sha384-")
        || lower.starts_with("sha512-")
    {
        return true;
    }
    is_uuid(value) || is_sha_like_hex(value) || is_jwt(value)
}

fn is_uuid(value: &str) -> bool {
    let parts: Vec<&str> = value.split('-').collect();
    parts.len() == 5
        && [8, 4, 4, 4, 12]
            .iter()
            .zip(parts.iter())
            .all(|(len, part)| part.len() == *len && part.chars().all(|ch| ch.is_ascii_hexdigit()))
}

fn is_sha_like_hex(value: &str) -> bool {
    matches!(value.len(), 32 | 40 | 64 | 128) && value.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn is_jwt(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| part.len() >= 8 && is_base64url(part))
}

fn is_base64url(value: &str) -> bool {
    value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_prefix_requires_full_shape() {
        assert_eq!(
            provider_secret_family("AKIA1234567890ABCDEF"),
            Some("AWS access key")
        );
        assert_eq!(provider_secret_family("AKIAshort"), None);
    }

    #[test]
    fn examples_and_hashes_are_allowlisted() {
        assert!(is_allowlisted_literal("AKIAIOSFODNN7EXAMPLE"));
        assert!(is_allowlisted_literal(
            "0123456789abcdef0123456789abcdef01234567"
        ));
        assert!(is_allowlisted_literal(
            "550e8400-e29b-41d4-a716-446655440000"
        ));
    }

    #[test]
    fn entropy_requires_secret_identifier() {
        assert_eq!(
            classify_secret_literal("cacheHash", "mF9a7Qp2Lx8Nz4Rv6Ts0"),
            None
        );
        assert_eq!(
            classify_secret_literal("WWW-Authenticate", "mF9a7Qp2Lx8Nz4Rv6Ts0"),
            None
        );
        assert_eq!(
            classify_secret_literal("apiKey", "mF9a7Qp2Lx8Nz4Rv6Ts0"),
            Some(SecretSignal::EntropyWithIdentifier)
        );
    }

    #[test]
    fn evidence_does_not_include_literal_value() {
        let value = "mF9a7Qp2Lx8Nz4Rv6Ts0";
        let evidence = redacted_evidence(
            classify_secret_literal("apiKey", value).expect("candidate"),
            "apiKey",
        );
        assert!(!evidence.contains(value));
    }
}
