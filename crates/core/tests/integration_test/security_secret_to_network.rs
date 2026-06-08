//! Integration tests for the `secret-to-network` exfil category (issue #890): a
//! non-public `process.env` / `import.meta.env` secret reaching a network sink
//! (`fetch` / `axios.*` / ...) via same-identifier source-backed flow.
//!
//! The category is INCLUDE-REQUIRED (it fires on intended auth as often as on
//! exfil), so it is admitted only when listed in `security.categories.include`.
//! Each candidate carries a destination-host signal (`candidate.network.destination`,
//! the arg-0 URL literal or absent for a dynamic destination) so a consuming
//! agent can triage exfil from intended auth.
//!
//! Findings are CANDIDATES for downstream agent verification, NOT verified
//! vulnerabilities.

use fallow_config::{SecurityCategories, SecurityConfig, Severity};
use fallow_core::results::{AnalysisResults, SecurityFinding};

use super::common::{create_config_with_rules, fixture_path};

const FIXTURE: &str = "security-secret-to-network";

/// Analyze the fixture with the `security_sink` rule on AND `secret-to-network`
/// explicitly included (the include-required opt-in).
fn analyze_included() -> AnalysisResults {
    let root = fixture_path(FIXTURE);
    let mut config = create_config_with_rules(root, |rules| {
        rules.security_sink = Severity::Warn;
    });
    config.security = SecurityConfig {
        categories: Some(SecurityCategories {
            include: Some(vec!["secret-to-network".to_string()]),
            exclude: None,
        }),
    };
    fallow_core::analyze(&config).expect("analysis should succeed")
}

/// Analyze the fixture with the rule on but WITHOUT the include (so the
/// include-required category must stay silent).
fn analyze_without_include() -> AnalysisResults {
    let root = fixture_path(FIXTURE);
    let config = create_config_with_rules(root, |rules| {
        rules.security_sink = Severity::Warn;
    });
    fallow_core::analyze(&config).expect("analysis should succeed")
}

fn secret_to_network_on<'a>(results: &'a AnalysisResults, suffix: &str) -> &'a SecurityFinding {
    results
        .security_findings
        .iter()
        .find(|f| {
            f.category.as_deref() == Some("secret-to-network")
                && f.path
                    .to_string_lossy()
                    .replace('\\', "/")
                    .ends_with(suffix)
        })
        .unwrap_or_else(|| panic!("{suffix} should produce a secret-to-network candidate"))
}

#[test]
fn process_env_secret_to_network_fires_with_cwe_and_source_kind() {
    let results = analyze_included();
    let finding = secret_to_network_on(&results, "src/exfil.ts");
    assert_eq!(finding.cwe, Some(201));
    assert_eq!(
        finding.candidate.source_kind.as_deref(),
        Some("process-env")
    );
}

#[test]
fn dynamic_destination_has_absent_network_destination() {
    // `fetch(resolveTarget(), ...)`: the destination is dynamic, the higher-signal
    // exfil case, so the candidate's network.destination is absent.
    let results = analyze_included();
    let finding = secret_to_network_on(&results, "src/exfil.ts");
    let network = finding
        .candidate
        .network
        .as_ref()
        .expect("network context present on a secret-to-network candidate");
    assert!(
        network.destination.is_none(),
        "a dynamic destination has no literal host"
    );
}

#[test]
fn literal_destination_carries_the_host() {
    // `fetch("https://api.stripe.com/...", ...)`: a literal provider host, usually
    // intended auth. The candidate carries the literal so the agent can triage.
    let results = analyze_included();
    let finding = secret_to_network_on(&results, "src/auth.ts");
    let network = finding.candidate.network.as_ref().expect("network context");
    assert_eq!(
        network.destination.as_deref(),
        Some("https://api.stripe.com/v1/charges")
    );
}

#[test]
fn import_meta_env_secret_is_source_backed() {
    // The Vite env surface (`import.meta.env.SERVER_API_KEY`) is a secret source
    // too (#890), so the same flow fires with the import-meta-env source kind.
    let results = analyze_included();
    let finding = secret_to_network_on(&results, "src/vite.ts");
    assert_eq!(
        finding.candidate.source_kind.as_deref(),
        Some("import-meta-env")
    );
}

#[test]
fn public_env_and_co_occurrence_do_not_fire() {
    // safe.ts holds three negatives: a public-prefix `process.env.NEXT_PUBLIC_*`
    // read, a `process.env` secret that is logged but never flows into the
    // request (co-occurrence only), and a public `import.meta.env.VITE_*` read.
    let results = analyze_included();
    assert!(
        !results.security_findings.iter().any(|f| {
            f.category.as_deref() == Some("secret-to-network")
                && f.path
                    .to_string_lossy()
                    .replace('\\', "/")
                    .ends_with("src/safe.ts")
        }),
        "public env vars and co-occurrence-only flows must not fire secret-to-network"
    );
}

#[test]
fn include_required_stays_silent_without_the_include() {
    // Without `security.categories.include`, the opt-in category never fires,
    // even though the rule is on and other default categories may.
    let results = analyze_without_include();
    assert!(
        !results
            .security_findings
            .iter()
            .any(|f| f.category.as_deref() == Some("secret-to-network")),
        "secret-to-network is include-required and must not fire without an explicit include"
    );
}
