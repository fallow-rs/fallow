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
    well_known_provider_family(value).or_else(|| prefixed_provider_family(value))
}

/// First-tier provider checks that rely on bespoke structural predicates
/// (AWS, GitHub, Slack, SendGrid, DigitalOcean, Telegram, PEM) plus the
/// highest-priority key prefixes. Checked before [`prefixed_provider_family`].
fn well_known_provider_family(value: &str) -> Option<&'static str> {
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
    None
}

/// Second-tier provider checks, predominantly fixed-prefix matches plus a few
/// bespoke predicates. Only reached when [`well_known_provider_family`] misses,
/// so first-match ordering across both tiers is preserved.
fn prefixed_provider_family(value: &str) -> Option<&'static str> {
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

    // --- existing tests ---

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

    // --- well_known_provider_family: lines 221-251 ---

    #[test]
    fn aws_akia_prefix_detected() {
        // AKIA + 16 uppercase alnum chars = 20 total
        assert_eq!(
            well_known_provider_family("AKIA1234567890ABCDEF"),
            Some("AWS access key")
        );
    }

    #[test]
    fn aws_asia_prefix_detected() {
        assert_eq!(
            well_known_provider_family("ASIA1234567890ABCDEF"),
            Some("AWS access key")
        );
    }

    #[test]
    fn aws_key_wrong_length_not_detected() {
        // 19 chars total (one short)
        assert_eq!(well_known_provider_family("AKIA1234567890ABCDE"), None);
    }

    #[test]
    fn github_ghp_token_detected() {
        // ghp_ + 36 alnum chars
        let token = format!("ghp_{}", "a".repeat(36));
        assert_eq!(well_known_provider_family(&token), Some("GitHub token"));
    }

    #[test]
    fn github_gho_token_detected() {
        let token = format!("gho_{}", "b".repeat(36));
        assert_eq!(well_known_provider_family(&token), Some("GitHub token"));
    }

    #[test]
    fn github_ghu_token_detected() {
        let token = format!("ghu_{}", "c".repeat(36));
        assert_eq!(well_known_provider_family(&token), Some("GitHub token"));
    }

    #[test]
    fn github_ghs_token_detected() {
        let token = format!("ghs_{}", "d".repeat(36));
        assert_eq!(well_known_provider_family(&token), Some("GitHub token"));
    }

    #[test]
    fn github_ghr_token_detected() {
        let token = format!("ghr_{}", "e".repeat(36));
        assert_eq!(well_known_provider_family(&token), Some("GitHub token"));
    }

    #[test]
    fn github_pat_long_form_detected() {
        // github_pat_ + at least 70 chars
        let token = format!("github_pat_{}", "x".repeat(70));
        assert_eq!(well_known_provider_family(&token), Some("GitHub token"));
    }

    #[test]
    fn github_token_too_short_not_detected() {
        // ghp_ + 35 chars (one short of required 36)
        let token = format!("ghp_{}", "a".repeat(35));
        assert_eq!(well_known_provider_family(&token), None);
    }

    #[test]
    fn gitlab_token_detected() {
        // glpat- + exactly 20 alnum chars
        let token = format!("glpat-{}", "a".repeat(20));
        assert_eq!(well_known_provider_family(&token), Some("GitLab token"));
    }

    #[test]
    fn gitlab_token_wrong_length_not_detected() {
        let token = format!("glpat-{}", "a".repeat(19));
        assert_eq!(well_known_provider_family(&token), None);
    }

    #[test]
    fn slack_xoxb_token_detected() {
        let token = format!("xoxb-{}", "a".repeat(16));
        assert_eq!(well_known_provider_family(&token), Some("Slack token"));
    }

    #[test]
    fn slack_xoxp_token_detected() {
        let token = format!("xoxp-{}", "b".repeat(16));
        assert_eq!(well_known_provider_family(&token), Some("Slack token"));
    }

    #[test]
    fn slack_xoxa_token_detected() {
        let token = format!("xoxa-{}", "c".repeat(16));
        assert_eq!(well_known_provider_family(&token), Some("Slack token"));
    }

    #[test]
    fn slack_xoxr_token_detected() {
        let token = format!("xoxr-{}", "d".repeat(16));
        assert_eq!(well_known_provider_family(&token), Some("Slack token"));
    }

    #[test]
    fn slack_xoxs_token_detected() {
        let token = format!("xoxs-{}", "e".repeat(16));
        assert_eq!(well_known_provider_family(&token), Some("Slack token"));
    }

    #[test]
    fn slack_token_too_short_not_detected() {
        // xoxb- + 11 chars (below min 16 tail)
        let token = format!("xoxb-{}", "a".repeat(11));
        assert_eq!(well_known_provider_family(&token), None);
    }

    #[test]
    fn stripe_sk_live_key_detected() {
        let key = format!("sk_live_{}", "a".repeat(16));
        assert_eq!(well_known_provider_family(&key), Some("Stripe key"));
    }

    #[test]
    fn stripe_rk_live_key_detected() {
        let key = format!("rk_live_{}", "b".repeat(16));
        assert_eq!(well_known_provider_family(&key), Some("Stripe key"));
    }

    #[test]
    fn anthropic_key_detected() {
        let key = format!("sk-ant-{}", "a".repeat(20));
        assert_eq!(well_known_provider_family(&key), Some("Anthropic key"));
    }

    #[test]
    fn openai_project_key_detected() {
        let key = format!("sk-proj-{}", "a".repeat(20));
        assert_eq!(well_known_provider_family(&key), Some("OpenAI project key"));
    }

    #[test]
    fn google_api_key_detected() {
        // AIza + exactly 35 alnum chars
        let key = format!("AIza{}", "a".repeat(35));
        assert_eq!(well_known_provider_family(&key), Some("Google API key"));
    }

    #[test]
    fn google_api_key_wrong_length_not_detected() {
        let key = format!("AIza{}", "a".repeat(34));
        assert_eq!(well_known_provider_family(&key), None);
    }

    #[test]
    fn sendgrid_key_detected() {
        // SG.<10+ chars>.<10+ chars>
        let key = format!("SG.{}.{}", "a".repeat(10), "b".repeat(10));
        assert_eq!(well_known_provider_family(&key), Some("SendGrid key"));
    }

    #[test]
    fn sendgrid_key_short_segment_not_detected() {
        // SG.<9 chars>.<10 chars> - first segment too short
        let key = format!("SG.{}.{}", "a".repeat(9), "b".repeat(10));
        assert_eq!(well_known_provider_family(&key), None);
    }

    // --- prefixed_provider_family: lines 256-301 ---

    #[test]
    fn npm_token_detected() {
        // npm_ + exactly 36 alnum chars
        let token = format!("npm_{}", "a".repeat(36));
        assert_eq!(prefixed_provider_family(&token), Some("npm token"));
    }

    #[test]
    fn npm_token_wrong_length_not_detected() {
        let token = format!("npm_{}", "a".repeat(35));
        assert_eq!(prefixed_provider_family(&token), None);
    }

    #[test]
    fn pypi_token_detected() {
        let token = format!("pypi-AgEI{}", "a".repeat(60));
        assert_eq!(prefixed_provider_family(&token), Some("PyPI token"));
    }

    #[test]
    fn square_eaaa_token_detected() {
        let token = format!("EAAA{}", "a".repeat(20));
        assert_eq!(prefixed_provider_family(&token), Some("Square token"));
    }

    #[test]
    fn square_sq0atp_token_detected() {
        let token = format!("sq0atp-{}", "a".repeat(20));
        assert_eq!(prefixed_provider_family(&token), Some("Square token"));
    }

    #[test]
    fn shopify_shpat_token_detected() {
        let token = format!("shpat_{}", "a".repeat(20));
        assert_eq!(prefixed_provider_family(&token), Some("Shopify token"));
    }

    #[test]
    fn shopify_shpss_token_detected() {
        let token = format!("shpss_{}", "b".repeat(20));
        assert_eq!(prefixed_provider_family(&token), Some("Shopify token"));
    }

    #[test]
    fn shopify_shpca_token_detected() {
        let token = format!("shpca_{}", "c".repeat(20));
        assert_eq!(prefixed_provider_family(&token), Some("Shopify token"));
    }

    #[test]
    fn shopify_shppa_token_detected() {
        let token = format!("shppa_{}", "d".repeat(20));
        assert_eq!(prefixed_provider_family(&token), Some("Shopify token"));
    }

    #[test]
    fn doppler_token_detected() {
        let token = format!("dp.pt.{}", "a".repeat(20));
        assert_eq!(prefixed_provider_family(&token), Some("Doppler token"));
    }

    #[test]
    fn digitalocean_dop_v1_token_detected() {
        // dop_v1_ + exactly 64 hex chars
        let token = format!("dop_v1_{}", "a".repeat(64));
        assert_eq!(prefixed_provider_family(&token), Some("DigitalOcean token"));
    }

    #[test]
    fn digitalocean_dor_v1_token_detected() {
        let token = format!("dor_v1_{}", "b".repeat(64));
        assert_eq!(prefixed_provider_family(&token), Some("DigitalOcean token"));
    }

    #[test]
    fn digitalocean_dot_v1_token_detected() {
        let token = format!("dot_v1_{}", "c".repeat(64));
        assert_eq!(prefixed_provider_family(&token), Some("DigitalOcean token"));
    }

    #[test]
    fn digitalocean_doo_v1_token_detected() {
        let token = format!("doo_v1_{}", "d".repeat(64));
        assert_eq!(prefixed_provider_family(&token), Some("DigitalOcean token"));
    }

    #[test]
    fn digitalocean_token_wrong_length_not_detected() {
        // 63 hex chars (one short)
        let token = format!("dop_v1_{}", "a".repeat(63));
        assert_eq!(prefixed_provider_family(&token), None);
    }

    #[test]
    fn digitalocean_token_non_hex_not_detected() {
        // 64 chars but contains 'g' (not hex)
        let token = format!("dop_v1_{}{}", "a".repeat(63), "g");
        assert_eq!(prefixed_provider_family(&token), None);
    }

    #[test]
    fn linear_token_detected() {
        let token = format!("lin_api_{}", "a".repeat(20));
        assert_eq!(prefixed_provider_family(&token), Some("Linear token"));
    }

    #[test]
    fn postman_key_detected() {
        let key = format!("PMAK-{}", "a".repeat(20));
        assert_eq!(prefixed_provider_family(&key), Some("Postman key"));
    }

    #[test]
    fn hugging_face_token_detected() {
        let token = format!("hf_{}", "a".repeat(20));
        assert_eq!(prefixed_provider_family(&token), Some("Hugging Face token"));
    }

    #[test]
    fn databricks_token_detected() {
        let token = format!("dapi{}", "a".repeat(20));
        assert_eq!(prefixed_provider_family(&token), Some("Databricks token"));
    }

    #[test]
    fn telegram_bot_token_detected() {
        // <8-10 digits>:AA<18+ chars>
        let token = "123456789:AAsynthetic_telegram_token_XYZ";
        assert_eq!(prefixed_provider_family(token), Some("Telegram bot token"));
    }

    #[test]
    fn telegram_bot_token_id_too_short_not_detected() {
        // Only 7 digits (below minimum 8)
        let token = "1234567:AAsynthetic_telegram_token_XYZ";
        assert_eq!(prefixed_provider_family(token), None);
    }

    #[test]
    fn telegram_bot_token_missing_aa_prefix_not_detected() {
        let token = "123456789:BBsynthetic_telegram_token_XYZ";
        assert_eq!(prefixed_provider_family(token), None);
    }

    #[test]
    fn telegram_bot_token_non_digit_id_not_detected() {
        let token = "12345678X:AAsynthetic_telegram_token_XYZ";
        assert_eq!(prefixed_provider_family(token), None);
    }

    #[test]
    fn age_secret_key_detected() {
        let key = format!("AGE-SECRET-KEY-1{}", "a".repeat(30));
        assert_eq!(prefixed_provider_family(&key), Some("age secret key"));
    }

    #[test]
    fn pem_private_key_detected() {
        let pem = "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIB...\n-----END RSA PRIVATE KEY-----";
        assert_eq!(prefixed_provider_family(pem), Some("PEM private key"));
    }

    #[test]
    fn pem_public_key_not_detected() {
        // PUBLIC KEY must not trigger the private-key check
        let pem = "-----BEGIN PUBLIC KEY-----\nMIIBIjAN...\n-----END PUBLIC KEY-----";
        assert_eq!(prefixed_provider_family(pem), None);
    }

    #[test]
    fn pem_private_key_missing_end_marker_not_detected() {
        // A certificate PEM has no "PRIVATE KEY-----" substring at all.
        let pem_no_end = "-----BEGIN CERTIFICATE-----\nMIICZjCC...";
        assert_eq!(prefixed_provider_family(pem_no_end), None);
    }

    // --- predicate functions: lines 326-399 ---

    #[test]
    fn is_aws_access_key_valid() {
        assert!(is_aws_access_key("AKIA1234567890ABCDEF"));
        assert!(is_aws_access_key("ASIA1234567890ABCDEF"));
    }

    #[test]
    fn is_aws_access_key_wrong_prefix() {
        assert!(!is_aws_access_key("AKIB1234567890ABCDEF"));
    }

    #[test]
    fn is_aws_access_key_lowercase_char_rejected() {
        // Lowercase 'a' in the tail violates the all-uppercase-or-digit rule
        assert!(!is_aws_access_key("AKIA1234567890ABCDEa"));
    }

    #[test]
    fn is_github_token_valid_short_form() {
        assert!(is_github_token(&format!("ghp_{}", "x".repeat(36))));
        assert!(is_github_token(&format!("gho_{}", "x".repeat(36))));
        assert!(is_github_token(&format!("ghu_{}", "x".repeat(36))));
        assert!(is_github_token(&format!("ghs_{}", "x".repeat(36))));
        assert!(is_github_token(&format!("ghr_{}", "x".repeat(36))));
    }

    #[test]
    fn is_github_token_valid_long_form() {
        assert!(is_github_token(&format!("github_pat_{}", "x".repeat(70))));
    }

    #[test]
    fn is_github_token_invalid_short_tail() {
        assert!(!is_github_token(&format!("ghp_{}", "x".repeat(35))));
    }

    #[test]
    fn is_slack_token_valid() {
        assert!(is_slack_token(&format!("xoxb-{}", "a".repeat(16))));
        assert!(is_slack_token(&format!("xoxp-{}", "a".repeat(16))));
        assert!(is_slack_token(&format!("xoxa-{}", "a".repeat(16))));
        assert!(is_slack_token(&format!("xoxr-{}", "a".repeat(16))));
        assert!(is_slack_token(&format!("xoxs-{}", "a".repeat(16))));
    }

    #[test]
    fn is_slack_token_invalid_prefix() {
        assert!(!is_slack_token("xoxz-synthetic_not_a_real_token_xx"));
    }

    #[test]
    fn is_sendgrid_key_valid() {
        assert!(is_sendgrid_key(&format!(
            "SG.{}.{}",
            "a".repeat(10),
            "b".repeat(10)
        )));
    }

    #[test]
    fn is_sendgrid_key_wrong_first_part() {
        // First part must literally be "SG"
        assert!(!is_sendgrid_key(&format!(
            "XX.{}.{}",
            "a".repeat(10),
            "b".repeat(10)
        )));
    }

    #[test]
    fn is_sendgrid_key_two_parts_only() {
        assert!(!is_sendgrid_key(&format!("SG.{}", "a".repeat(10))));
    }

    #[test]
    fn is_digitalocean_token_valid() {
        assert!(is_digitalocean_token(&format!("dop_v1_{}", "a".repeat(64))));
        assert!(is_digitalocean_token(&format!("dor_v1_{}", "b".repeat(64))));
        assert!(is_digitalocean_token(&format!("dot_v1_{}", "c".repeat(64))));
        assert!(is_digitalocean_token(&format!("doo_v1_{}", "d".repeat(64))));
    }

    #[test]
    fn is_digitalocean_token_non_hex_tail() {
        // 'z' is not a hex digit
        let token = format!("dop_v1_{}{}", "a".repeat(63), "z");
        assert!(!is_digitalocean_token(&token));
    }

    #[test]
    fn is_telegram_bot_token_valid() {
        assert!(is_telegram_bot_token(
            "123456789:AAsynthetic_telegram_token_XYZ"
        ));
    }

    #[test]
    fn is_telegram_bot_token_id_length_boundaries() {
        // Minimum valid ID length is 8 digits
        assert!(is_telegram_bot_token(
            "12345678:AAsynthetic_token_XYZ_extra"
        ));
        // Maximum valid ID length is 10 digits
        assert!(is_telegram_bot_token(
            "1234567890:AAsynthetic_token_XYZ_extra"
        ));
        // 11 digits is too long
        assert!(!is_telegram_bot_token(
            "12345678901:AAsynthetic_token_XYZ_extra"
        ));
    }

    #[test]
    fn is_telegram_bot_token_no_colon() {
        assert!(!is_telegram_bot_token("123456789AAsynthetic_token_nocolon"));
    }

    #[test]
    fn is_pem_private_key_valid() {
        let pem = "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIB...\n-----END RSA PRIVATE KEY-----";
        assert!(is_pem_private_key(pem));
    }

    #[test]
    fn is_pem_private_key_public_key_rejected() {
        let pem = "-----BEGIN PUBLIC KEY-----\nMIIBIjAN...\n-----END PUBLIC KEY-----";
        assert!(!is_pem_private_key(pem));
    }

    #[test]
    fn is_pem_private_key_missing_begin_rejected() {
        let pem = "MIIE PRIVATE KEY----- some stuff";
        assert!(!is_pem_private_key(pem));
    }

    // --- is_jwt / is_base64url: lines 456-475 ---

    #[test]
    fn is_jwt_valid_three_part_base64url() {
        // Three base64url segments all at least 8 chars long
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ0ZXN0In0.SyntheticSignatureXY";
        assert!(is_jwt(jwt));
    }

    #[test]
    fn is_jwt_two_parts_not_a_jwt() {
        let not_jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ0ZXN0In0";
        assert!(!is_jwt(not_jwt));
    }

    #[test]
    fn is_jwt_four_parts_not_a_jwt() {
        let not_jwt = "a.b.c.d";
        assert!(!is_jwt(not_jwt));
    }

    #[test]
    fn is_jwt_part_too_short_not_a_jwt() {
        // Third segment is only 7 chars (below minimum 8)
        let not_jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ0ZXN0In0.short7";
        assert!(!is_jwt(not_jwt));
    }

    #[test]
    fn is_jwt_non_base64url_chars_rejected() {
        // Plus and slash are NOT base64url (they are standard base64)
        let not_jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ0ZXN0In0.SYnthEtIcSign+tur/Z";
        assert!(!is_jwt(not_jwt));
    }

    #[test]
    fn is_base64url_valid_chars() {
        assert!(is_base64url("abcdefABCDEF0123456789-_"));
    }

    #[test]
    fn is_base64url_rejects_plus_and_slash() {
        assert!(!is_base64url("abc+def"));
        assert!(!is_base64url("abc/def"));
    }

    // --- allowlist edge cases ---

    #[test]
    fn placeholder_variants_are_allowlisted() {
        assert!(is_allowlisted_literal("my-placeholder-value"));
        assert!(is_allowlisted_literal("changeme_now"));
        assert!(is_allowlisted_literal("dummy_secret_for_tests"));
    }

    #[test]
    fn data_uri_is_allowlisted() {
        assert!(is_allowlisted_literal(
            "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAUA"
        ));
    }

    #[test]
    fn sha_integrity_hashes_are_allowlisted() {
        assert!(is_allowlisted_literal(
            "sha256-47DEQpj8HBSa-_TImW-5JCeuQeRkm5NMpJWZG3hSuFU="
        ));
        assert!(is_allowlisted_literal(
            "sha384-H8BRh8j48O9oEatShhK7gg1ggPmRDvHjYRF9m0e4U8="
        ));
        assert!(is_allowlisted_literal(
            "sha512-Q2bFTOhEALkN8hOms2FKTDLy7eugP2zFZ1T8LCvX42Cdh7AFsbIIGh=="
        ));
    }

    #[test]
    fn jwt_shaped_value_is_allowlisted() {
        // A syntactically valid JWT is allowlisted (no false positives)
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ0ZXN0In0.SyntheticSignatureXY";
        assert!(is_allowlisted_literal(jwt));
    }

    #[test]
    fn sha_like_hex_lengths_are_allowlisted() {
        // 32, 40, 64, 128 hex chars are SHA-like
        assert!(is_allowlisted_literal(&"a".repeat(32)));
        assert!(is_allowlisted_literal(&"b".repeat(40)));
        assert!(is_allowlisted_literal(&"c".repeat(64)));
        assert!(is_allowlisted_literal(&"d".repeat(128)));
    }

    #[test]
    fn non_sha_hex_length_not_allowlisted_by_hex_rule() {
        // 33 all-hex chars: not a recognized SHA length
        // (may still be allowlisted by another rule, but not the hex one)
        assert!(!is_sha_like_hex(&"a".repeat(33)));
    }

    // --- secret-shaped identifier keywords ---

    #[test]
    fn secret_identifier_keywords_matched() {
        assert!(is_secret_shaped_identifier("apiKey"));
        assert!(is_secret_shaped_identifier("api_key"));
        assert!(is_secret_shaped_identifier("accessKey"));
        assert!(is_secret_shaped_identifier("access_key"));
        assert!(is_secret_shaped_identifier("privateKey"));
        assert!(is_secret_shaped_identifier("private_key"));
        assert!(is_secret_shaped_identifier("clientSecret"));
        assert!(is_secret_shaped_identifier("client_secret"));
        assert!(is_secret_shaped_identifier("token"));
        assert!(is_secret_shaped_identifier("secret"));
        assert!(is_secret_shaped_identifier("password"));
        assert!(is_secret_shaped_identifier("passwd"));
        assert!(is_secret_shaped_identifier("credential"));
        assert!(is_secret_shaped_identifier("jwt"));
    }

    #[test]
    fn non_secret_identifier_not_matched() {
        assert!(!is_secret_shaped_identifier("cacheHash"));
        assert!(!is_secret_shaped_identifier("userId"));
        assert!(!is_secret_shaped_identifier("displayName"));
    }

    // --- entropy / mixed-charset helpers ---

    #[test]
    fn high_entropy_requires_minimum_length() {
        // A 19-char value (below MIN_ENTROPY_LENGTH=20) is never high entropy
        assert!(!has_high_entropy("aB3-xY7mQp1Lz9Rv5Ts"));
    }

    #[test]
    fn high_entropy_requires_mixed_charset() {
        // 20 identical chars: low entropy regardless of charset
        assert!(!has_high_entropy(&"a".repeat(20)));
    }

    #[test]
    fn has_mixed_secret_charset_needs_three_categories() {
        // Only lowercase + digits (2 categories) should not satisfy the 3-of-4 gate
        assert!(!has_mixed_secret_charset("abcdef1234567890abcd"));
        // Lower + upper + digits (3 categories) is enough
        assert!(has_mixed_secret_charset("abcABC123456789abcAB"));
    }

    // --- classify_secret_literal: allowlisted values short-circuit ---

    #[test]
    fn allowlisted_value_short_circuits_classifier() {
        // "example" in the value makes it allowlisted, even with a secret identifier
        assert_eq!(
            classify_secret_literal("apiKey", "some-example-key-value-here"),
            None
        );
    }

    #[test]
    fn provider_signal_takes_priority_over_entropy() {
        // An AWS key shape should yield Provider, not EntropyWithIdentifier
        let aws = "AKIA1234567890ABCDEF";
        let result = classify_secret_literal("apiKey", aws);
        assert_eq!(result, Some(SecretSignal::Provider("AWS access key")));
    }

    // --- evidence text shape ---

    #[test]
    fn provider_evidence_names_provider() {
        let evidence = redacted_evidence(SecretSignal::Provider("Stripe key"), "sk_live_foo");
        assert!(evidence.contains("Stripe key"));
        assert!(!evidence.contains("sk_live_foo"));
    }

    #[test]
    fn entropy_evidence_names_identifier() {
        let evidence = redacted_evidence(SecretSignal::EntropyWithIdentifier, "mySecretToken");
        assert!(evidence.contains("mySecretToken"));
    }

    // --- is_token_tail: internal validation helper ---

    #[test]
    fn token_tail_allows_alphanumeric_and_special_chars() {
        assert!(is_token_tail("abc123ABC-_.."));
    }

    #[test]
    fn token_tail_rejects_space_and_at() {
        assert!(!is_token_tail("abc def"));
        assert!(!is_token_tail("abc@def"));
    }
}
