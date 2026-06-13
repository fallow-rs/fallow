use fallow_config::{FallowConfig, OutputFormat, RulesConfig, Severity};

use crate::common::fixture_path;

/// Resolve a fixture with the `mixed-client-server-barrel` rule at `warn` (its
/// default). The detector is gated on the project declaring `next`, which the
/// positive fixture's `package.json` does.
fn fixture_config(name: &str) -> fallow_config::ResolvedConfig {
    FallowConfig {
        rules: RulesConfig {
            mixed_client_server_barrel: Severity::Warn,
            ..RulesConfig::default()
        },
        ..Default::default()
    }
    .resolve(fixture_path(name), OutputFormat::Human, 4, true, true, None)
}

fn barrel_paths(results: &fallow_core::results::AnalysisResults) -> Vec<String> {
    results
        .mixed_client_server_barrels
        .iter()
        .map(|f| f.barrel.path.to_string_lossy().replace('\\', "/"))
        .collect()
}

#[test]
fn client_and_server_only_barrel_is_flagged_once() {
    let config = fixture_config("mixed-client-server-barrel");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let offending: Vec<&fallow_core::results::MixedClientServerBarrelFinding> = results
        .mixed_client_server_barrels
        .iter()
        .filter(|f| {
            f.barrel
                .path
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with("app/components/index.ts")
        })
        .collect();

    assert_eq!(
        offending.len(),
        1,
        "exactly one mixed client/server barrel expected at app/components/index.ts: {:?}",
        barrel_paths(&results)
    );

    // The origins are the re-export source specifiers as written in the barrel.
    let finding = offending[0];
    assert_eq!(finding.barrel.client_origin, "./Button");
    assert_eq!(finding.barrel.server_origin, "./fetchUser");
}

#[test]
fn client_plus_plain_util_barrel_is_not_flagged() {
    // LOAD-BEARING FALSE-POSITIVE GUARD: a barrel re-exporting a "use client"
    // component alongside an ordinary undirected utility module must NOT flag.
    // The trigger is client + SERVER-ONLY, never client + plain-util.
    let config = fixture_config("mixed-client-server-barrel");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        !results.mixed_client_server_barrels.iter().any(|f| f
            .barrel
            .path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("app/safe-barrel/index.ts")),
        "client + plain-util barrel must not produce a finding: {:?}",
        barrel_paths(&results)
    );
}

#[test]
fn type_only_client_reexport_alongside_server_is_not_flagged() {
    // A `export type { X } from './ClientTypes'` re-export is erased and carries
    // no runtime directive context, so a type-only client re-export alongside a
    // server-only value re-export is NOT a client/server mix.
    let config = fixture_config("mixed-client-server-barrel");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        !results.mixed_client_server_barrels.iter().any(|f| f
            .barrel
            .path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("app/type-barrel/index.ts")),
        "type-only client re-export alongside a server-only re-export must not flag: {:?}",
        barrel_paths(&results)
    );
}

#[test]
fn no_findings_when_next_is_absent() {
    let config = fixture_config("mixed-client-server-barrel-no-next");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.mixed_client_server_barrels.is_empty(),
        "without `next` declared, the rule must not fire: {:?}",
        barrel_paths(&results)
    );
}
