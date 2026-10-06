use fallow_config::{FallowConfig, OutputFormat, RulesConfig, Severity};
use fallow_types::results::{PolicyRuleKind, PolicyViolationSeverity};

use crate::common::fixture_path;

/// Resolve the rule-packs fixture with the team-policy pack loaded from disk
/// (the same path `resolve()` takes for a real `rulePacks` config entry).
fn fixture_config(rule_packs: Vec<String>) -> fallow_config::ResolvedConfig {
    FallowConfig {
        entry: vec!["src/index.ts".to_string()],
        rules: RulesConfig {
            policy_violation: Severity::Warn,
            ..RulesConfig::default()
        },
        rule_packs,
        ..Default::default()
    }
    .resolve(
        fixture_path("rule-packs"),
        OutputFormat::Human,
        4,
        true,
        true,
        None,
    )
}

#[test]
fn rule_pack_reports_banned_calls_imports_and_effects_end_to_end() {
    let config = fixture_config(vec!["packs/team-policy.jsonc".to_string()]);
    assert_eq!(config.rule_packs.len(), 1, "pack should load from disk");

    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let by_rule: Vec<(&str, &str, PolicyRuleKind, PolicyViolationSeverity, String)> = results
        .policy_violations
        .iter()
        .map(|f| {
            (
                f.violation.rule_id.as_str(),
                f.violation.matched.as_str(),
                f.violation.kind,
                f.violation.severity,
                f.violation.path.to_string_lossy().replace('\\', "/"),
            )
        })
        .collect();

    // The literal-arg call in index.ts fires with the rule-level error
    // severity overriding the warn master.
    let banned_call: Vec<_> = by_rule
        .iter()
        .filter(|(rule, ..)| *rule == "no-child-process")
        .collect();
    assert_eq!(
        banned_call.len(),
        1,
        "exactly one banned-call finding expected: {by_rule:?}"
    );
    assert_eq!(banned_call[0].1, "execSync");
    assert_eq!(banned_call[0].2, PolicyRuleKind::BannedCall);
    assert_eq!(banned_call[0].3, PolicyViolationSeverity::Error);
    assert!(banned_call[0].4.ends_with("src/index.ts"));

    // Banned imports: the value import and the subpath import fire with the
    // warn master; the type-only import (ignoreTypeOnly) and moment-timezone
    // (segment-aware) stay quiet.
    let banned_imports: Vec<_> = by_rule
        .iter()
        .filter(|(rule, ..)| *rule == "no-moment")
        .collect();
    let matched: Vec<&str> = banned_imports.iter().map(|entry| entry.1).collect();
    assert_eq!(
        matched,
        vec!["moment", "moment/locale/nl"],
        "segment-aware import matching: {by_rule:?}"
    );
    assert!(
        banned_imports
            .iter()
            .all(|entry| entry.3 == PolicyViolationSeverity::Warn)
    );

    let banned_effects: Vec<_> = by_rule
        .iter()
        .filter(|(rule, ..)| *rule == "no-network")
        .collect();
    assert_eq!(
        banned_effects.len(),
        1,
        "exactly one banned-effect finding expected: {by_rule:?}"
    );
    assert_eq!(banned_effects[0].1, "network: fetch");
    assert_eq!(banned_effects[0].2, PolicyRuleKind::BannedEffect);
    assert_eq!(banned_effects[0].3, PolicyViolationSeverity::Warn);

    // Nothing else fires: the suppressed call is consumed (not stale) and the
    // tooling file is excluded by the rule's glob.
    assert_eq!(results.policy_violations.len(), 4, "{by_rule:?}");
    assert!(
        results
            .stale_suppressions
            .iter()
            .all(|s| !s.path.ends_with("suppressed.ts")),
        "consumed policy suppression must not be stale: {:?}",
        results.stale_suppressions
    );

    // Counted toward the run total.
    assert!(results.total_issues() >= 4);
}

#[test]
fn no_rule_packs_configured_means_zero_policy_findings() {
    let config = fixture_config(Vec::new());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(results.policy_violations.is_empty());
}

#[test]
fn master_off_disables_the_evaluator_entirely() {
    let mut config = fixture_config(vec!["packs/team-policy.jsonc".to_string()]);
    config.rules.policy_violation = Severity::Off;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(
        results.policy_violations.is_empty(),
        "master off is a kill switch even for severity: error rules"
    );
}

/// Public analysis seam for proof policies, including files without an entry path.
fn analyze_gdp_project(
    files: &[(&str, &str)],
    rules: serde_json::Value,
) -> fallow_types::results::AnalysisResults {
    analyze_gdp_project_with_config(files, rules, |_| {})
}

fn analyze_gdp_project_with_config(
    files: &[(&str, &str)],
    rules: serde_json::Value,
    configure: impl FnOnce(&mut FallowConfig),
) -> fallow_types::results::AnalysisResults {
    let dir = tempfile::tempdir().expect("temporary project");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"gdp-policy-fixture","type":"module","dependencies":{"@gdp-ts/core":"*"}}"#,
    )
    .unwrap();
    for (path, source) in files {
        let target = dir.path().join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, source).unwrap();
    }
    let mut pack = serde_json::json!({"version":1,"name":"proof-policy"});
    pack["rules"] = rules;
    std::fs::write(dir.path().join("pack.json"), pack.to_string()).unwrap();
    let mut unresolved = FallowConfig {
        entry: vec!["src/index.ts".into()],
        rules: RulesConfig {
            policy_violation: Severity::Warn,
            ..RulesConfig::default()
        },
        rule_packs: vec!["pack.json".into()],
        ..Default::default()
    };
    configure(&mut unresolved);
    let config = unresolved.resolve(
        dir.path().to_path_buf(),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    fallow_core::analyze(&config).expect("proof project analysis")
}

#[test]
fn gdp_producer_checks_direct_alias_and_unreachable_factory_sites() {
    let results = analyze_gdp_project(
        &[
            (
                "src/index.ts",
                "import { defineProof as factory } from '@gdp-ts/core'; factory('CanDeleteProject'); factory('CanDeleteProject');",
            ),
            (
                "src/orphan.ts",
                "import * as gdp from '@gdp-ts/core'; gdp.defineProof('CanReadProject');",
            ),
            (
                "src/trusted/auth.ts",
                "import { defineProof } from '@gdp-ts/core'; defineProof('CanDeleteProject');",
            ),
        ],
        serde_json::json!([{"id":"trusted-producers","kind":"gdp-proof-producer","allowedFiles":["src/trusted/**"]}]),
    );
    let findings = &results.policy_violations;
    assert_eq!(
        findings.len(),
        3,
        "each unauthorized producer site must be reported: {findings:?}"
    );
    assert_eq!(
        findings
            .iter()
            .filter(|finding| finding.violation.path.ends_with("src/index.ts"))
            .count(),
        2
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding.violation.path.ends_with("src/orphan.ts"))
    );
    assert_eq!(
        findings[0].violation.matched,
        "@gdp-ts/core.defineProof(\"CanDeleteProject\")"
    );
    assert_ne!(findings[0].violation.col, findings[1].violation.col);
    assert!(
        findings[0]
            .violation
            .message
            .as_deref()
            .unwrap()
            .contains("src/trusted/**")
    );
}

#[test]
fn gdp_producer_follows_named_star_default_and_namespace_forwarding() {
    let results = analyze_gdp_project(
        &[
            (
                "src/index.ts",
                "import { mint } from './barrel'; import factory from './default'; import * as nested from './namespace'; import { localFactory } from './local'; mint('Named'); factory('Default'); nested.gdp.defineProof('Namespace'); localFactory('LocalExport');",
            ),
            (
                "src/orphan.ts",
                "import { mint } from './barrel'; mint('OrphanNamed');",
            ),
            ("src/barrel.ts", "export * from './named';"),
            (
                "src/named.ts",
                "export { defineProof as mint } from '@gdp-ts/core';",
            ),
            (
                "src/default.ts",
                "export { defineProof as default } from '@gdp-ts/core';",
            ),
            ("src/namespace.ts", "export * as gdp from '@gdp-ts/core';"),
            (
                "src/local.ts",
                "import { defineProof as local } from '@gdp-ts/core'; export { local as localFactory };",
            ),
        ],
        serde_json::json!([{"id":"trusted-producers","kind":"gdp-proof-producer","allowedFiles":["src/trusted/**"]}]),
    );
    let matched: Vec<_> = results
        .policy_violations
        .iter()
        .map(|finding| finding.violation.matched.as_str())
        .collect();
    assert_eq!(
        matched,
        [
            "@gdp-ts/core.defineProof(\"Named\")",
            "@gdp-ts/core.defineProof(\"Default\")",
            "@gdp-ts/core.defineProof(\"Namespace\")",
            "@gdp-ts/core.defineProof(\"LocalExport\")",
            "@gdp-ts/core.defineProof(\"OrphanNamed\")"
        ]
    );
}

#[test]
fn gdp_producer_abstains_on_shadowed_ambiguous_type_only_and_cyclic_origins() {
    let results = analyze_gdp_project(
        &[
            (
                "src/index.ts",
                "import { defineProof } from '@gdp-ts/core'; import { defineProof as ambiguous } from './ambiguous'; import { defineProof as overridden } from './overridden'; import { defineProof as cyclic } from './cycle-a'; import type { defineProof as erased } from '@gdp-ts/core'; import { defineProof as single } from './single'; defineProof('PositiveControl'); single('Unambiguous'); function scoped(defineProof) { defineProof('Shadowed'); } ambiguous('Ambiguous'); overridden('LocalOverride'); cyclic('Cycle'); erased('TypeOnly');",
            ),
            ("src/single.ts", "export * from './genuine';"),
            (
                "src/ambiguous.ts",
                "export * from './genuine'; export * from './unrelated';",
            ),
            (
                "src/genuine.ts",
                "export { defineProof } from '@gdp-ts/core';",
            ),
            (
                "src/unrelated.ts",
                "export const defineProof = (kind) => kind;",
            ),
            (
                "src/overridden.ts",
                "export * from './genuine'; export const defineProof = (kind) => kind;",
            ),
            ("src/cycle-a.ts", "export * from './cycle-b';"),
            ("src/cycle-b.ts", "export * from './cycle-a';"),
        ],
        serde_json::json!([{"id":"trusted-producers","kind":"gdp-proof-producer","allowedFiles":["src/trusted/**"]}]),
    );
    assert_eq!(
        results
            .policy_violations
            .iter()
            .map(|finding| finding.violation.matched.as_str())
            .collect::<Vec<_>>(),
        [
            "@gdp-ts/core.defineProof(\"PositiveControl\")",
            "@gdp-ts/core.defineProof(\"Unambiguous\")"
        ]
    );
}

#[test]
fn gdp_producer_retains_independent_rules_dynamic_kinds_and_qualified_suppressions() {
    let results = analyze_gdp_project(
        &[
            (
                "src/index.ts",
                "import { defineProof } from '@gdp-ts/core';\ndefineProof('CanDelete'); defineProof(getKind()); defineProof('...');\n// fallow-ignore-next-line policy-violation:proof-policy/delete-owner\ndefineProof('CanDelete');",
            ),
            (
                "src/trusted/wrong-owner.ts",
                "import { defineProof } from '@gdp-ts/core'; defineProof('CanDelete'); defineProof('CanRead');",
            ),
            (
                "src/trusted/delete.ts",
                "import { defineProof } from '@gdp-ts/core'; defineProof('CanDelete');",
            ),
        ],
        serde_json::json!([
            {"id":"trusted-producers","kind":"gdp-proof-producer","allowedFiles":["src/trusted/**"]},
            {"id":"delete-owner","kind":"gdp-proof-producer","allowedFiles":["src/trusted/delete.ts"],"proofKinds":["CanDelete"],"severity":"error","message":"Use the delete owner."}
        ]),
    );
    let mut findings: Vec<_> = results
        .policy_violations
        .iter()
        .map(|finding| {
            (
                finding.violation.rule_id.as_str(),
                finding.violation.matched.as_str(),
                finding.violation.severity,
            )
        })
        .collect();
    let mut expected = vec![
        (
            "trusted-producers",
            "@gdp-ts/core.defineProof(\"CanDelete\")",
            PolicyViolationSeverity::Warn,
        ),
        (
            "delete-owner",
            "@gdp-ts/core.defineProof(\"CanDelete\")",
            PolicyViolationSeverity::Error,
        ),
        (
            "trusted-producers",
            "@gdp-ts/core.defineProof(...)",
            PolicyViolationSeverity::Warn,
        ),
        (
            "trusted-producers",
            "@gdp-ts/core.defineProof(\"...\")",
            PolicyViolationSeverity::Warn,
        ),
        (
            "trusted-producers",
            "@gdp-ts/core.defineProof(\"CanDelete\")",
            PolicyViolationSeverity::Warn,
        ),
        (
            "delete-owner",
            "@gdp-ts/core.defineProof(\"CanDelete\")",
            PolicyViolationSeverity::Error,
        ),
    ];
    findings.sort_by_key(|entry| (entry.0, entry.1, entry.2 == PolicyViolationSeverity::Error));
    expected.sort_by_key(|entry| (entry.0, entry.1, entry.2 == PolicyViolationSeverity::Error));
    assert_eq!(findings, expected);
    assert_eq!(
        results
            .policy_violations
            .iter()
            .find(|finding| finding.violation.rule_id == "delete-owner")
            .unwrap()
            .violation
            .message
            .as_deref(),
        Some("Use the delete owner.")
    );
    assert!(
        results.stale_suppressions.is_empty(),
        "matched qualified suppression must be consumed"
    );
}

#[test]
fn gdp_producer_follows_workspace_and_local_default_exports() {
    let results = analyze_gdp_project(
        &[
            (
                "package.json",
                r#"{"name":"workspace-proof-policy","type":"module","workspaces":["packages/*"],"dependencies":{"@gdp-ts/core":"*"}}"#,
            ),
            (
                "packages/proofs/package.json",
                r#"{"name":"@example/proofs","exports":"./src/index.ts","type":"module"}"#,
            ),
            (
                "packages/proofs/src/index.ts",
                "import { defineProof as factory } from '@gdp-ts/core'; export default factory; export { factory as mint };",
            ),
            (
                "src/index.ts",
                "import factory, { mint } from '@example/proofs'; factory('DefaultWorkspace'); mint('NamedWorkspace');",
            ),
        ],
        serde_json::json!([{"id":"trusted-producers","kind":"gdp-proof-producer","allowedFiles":["src/trusted/**"]}]),
    );
    assert_eq!(
        results
            .policy_violations
            .iter()
            .map(|finding| finding.violation.matched.as_str())
            .collect::<Vec<_>>(),
        [
            "@gdp-ts/core.defineProof(\"DefaultWorkspace\")",
            "@gdp-ts/core.defineProof(\"NamedWorkspace\")"
        ]
    );
}

#[test]
fn gdp_producer_scope_and_disabled_modes_keep_positive_controls() {
    let files = [
        (
            "src/index.ts",
            "import { defineProof } from '@gdp-ts/core'; defineProof('Included');",
        ),
        (
            "src/excluded.ts",
            "import { defineProof } from '@gdp-ts/core'; defineProof('Excluded');",
        ),
        (
            "src/legacy/producer.ts",
            "import { defineProof } from '@gdp-ts/core'; defineProof('MasterOff');",
        ),
        (
            "other/outside.ts",
            "import { defineProof } from '@gdp-ts/core'; defineProof('OutsideScope');",
        ),
    ];
    let rules = serde_json::json!([{ "id":"scoped", "kind":"gdp-proof-producer", "allowedFiles":["src/trusted/**"], "files":["src/**"], "exclude":["src/excluded.ts"], "zones":["application"], "severity":"error" }]);
    let results = analyze_gdp_project_with_config(&files, rules.clone(), |config| {
        config.boundaries = serde_json::from_value(
            serde_json::json!({"zones":[{"name":"application","patterns":["src/**"]}]}),
        )
        .unwrap();
        config.overrides = serde_json::from_value(
            serde_json::json!([{ "files":["src/legacy/**"], "rules":{"policy-violation":"off"} }]),
        )
        .unwrap();
    });
    assert_eq!(
        results
            .policy_violations
            .iter()
            .map(|finding| finding.violation.matched.as_str())
            .collect::<Vec<_>>(),
        ["@gdp-ts/core.defineProof(\"Included\")"]
    );
    let off = analyze_gdp_project_with_config(&files, rules.clone(), |config| {
        config.rules.policy_violation = Severity::Off;
    });
    assert!(off.policy_violations.is_empty());
    let no_pack =
        analyze_gdp_project_with_config(&files, rules, |config| config.rule_packs.clear());
    assert!(no_pack.policy_violations.is_empty());
    let rule_off = analyze_gdp_project(
        &files,
        serde_json::json!([{ "id":"disabled", "kind":"gdp-proof-producer", "allowedFiles":["src/trusted/**"], "severity":"off" }]),
    );
    assert!(rule_off.policy_violations.is_empty());
}

#[test]
fn gdp_producer_recognizes_the_actual_core_self_package_api() {
    let results = analyze_gdp_project(
        &[
            (
                "package.json",
                r#"{"name":"@gdp-ts/core","type":"module","exports":{".":"./src/factory.ts"}}"#,
            ),
            (
                "src/factory.ts",
                "export const defineProof = (kind) => ({kind});",
            ),
            (
                "src/index.ts",
                "import { defineProof } from '@gdp-ts/core'; defineProof('SelfPackage');",
            ),
        ],
        serde_json::json!([{"id":"trusted-producers","kind":"gdp-proof-producer","allowedFiles":["src/trusted/**"]}]),
    );
    assert_eq!(
        results
            .policy_violations
            .iter()
            .map(|finding| finding.violation.matched.as_str())
            .collect::<Vec<_>>(),
        ["@gdp-ts/core.defineProof(\"SelfPackage\")"]
    );
}

#[test]
fn gdp_producer_abstains_on_excessive_export_depth_without_losing_direct_control() {
    let mut files = vec![("src/index.ts".to_owned(), "import { defineProof } from '@gdp-ts/core'; import { defineProof as deep } from './chain0'; defineProof('Direct'); deep('Deep');".to_owned())];
    for index in 0..256 {
        let source = if index == 255 {
            "export { defineProof } from '@gdp-ts/core';".to_owned()
        } else {
            format!("export {{ defineProof }} from './chain{}';", index + 1)
        };
        files.push((format!("src/chain{index}.ts"), source));
    }
    let borrowed: Vec<_> = files
        .iter()
        .map(|(path, source)| (path.as_str(), source.as_str()))
        .collect();
    let results = analyze_gdp_project(
        &borrowed,
        serde_json::json!([{"id":"trusted-producers","kind":"gdp-proof-producer","allowedFiles":["src/trusted/**"]}]),
    );
    assert_eq!(
        results
            .policy_violations
            .iter()
            .map(|finding| finding.violation.matched.as_str())
            .collect::<Vec<_>>(),
        ["@gdp-ts/core.defineProof(\"Direct\")"]
    );
}

#[test]
fn gdp_producer_does_not_mistake_a_tsconfig_alias_for_the_core_package() {
    let rules = serde_json::json!([{"id":"trusted-producers","kind":"gdp-proof-producer","allowedFiles":["src/trusted/**"]}]);
    let entry = "import { defineProof } from '@gdp-ts/core'; defineProof('Control');";
    let genuine = analyze_gdp_project(&[("src/index.ts", entry)], rules.clone());
    assert_eq!(
        genuine
            .policy_violations
            .iter()
            .map(|finding| finding.violation.matched.as_str())
            .collect::<Vec<_>>(),
        ["@gdp-ts/core.defineProof(\"Control\")"]
    );
    let alias = analyze_gdp_project(
        &[
            ("src/index.ts", entry),
            ("src/fake.ts", "export const defineProof = (kind) => kind;"),
            (
                "tsconfig.json",
                r#"{"compilerOptions":{"baseUrl":".","paths":{"@gdp-ts/core":["src/fake.ts"]}}}"#,
            ),
        ],
        rules,
    );
    assert!(
        alias.policy_violations.is_empty(),
        "a local path alias is not the library package API"
    );
}

#[test]
fn gdp_producer_distinguishes_string_named_exports_from_namespace_members() {
    let results = analyze_gdp_project(
        &[
            (
                "src/index.ts",
                "import { \"x.defineProof\" as unrelated, x } from './bridge'; unrelated('LocalStringExport'); x.defineProof('NamespaceControl');",
            ),
            (
                "src/bridge.ts",
                "export * as x from '@gdp-ts/core'; const local = (kind) => kind; export { local as \"x.defineProof\" };",
            ),
        ],
        serde_json::json!([{"id":"trusted-producers","kind":"gdp-proof-producer","allowedFiles":["src/trusted/**"]}]),
    );
    assert_eq!(
        results
            .policy_violations
            .iter()
            .map(|finding| finding.violation.matched.as_str())
            .collect::<Vec<_>>(),
        ["@gdp-ts/core.defineProof(\"NamespaceControl\")"]
    );
}

#[test]
fn gdp_producer_distinguishes_quoted_star_from_whole_module_exports() {
    let results = analyze_gdp_project(
        &[
            (
                "src/index.ts",
                "import { \"*\" as unrelated, genuine } from './bridge'; unrelated.defineProof('LocalStarExport'); genuine.defineProof('NamespaceStarControl');",
            ),
            (
                "src/other.ts",
                "const local = { defineProof: (kind) => kind }; export { local as \"*\" }; export { defineProof } from '@gdp-ts/core';",
            ),
            (
                "src/bridge.ts",
                "export { \"*\" } from './other'; export * as genuine from '@gdp-ts/core';",
            ),
        ],
        serde_json::json!([{"id":"trusted-producers","kind":"gdp-proof-producer","allowedFiles":["src/trusted/**"]}]),
    );
    assert_eq!(
        results
            .policy_violations
            .iter()
            .map(|finding| finding.violation.matched.as_str())
            .collect::<Vec<_>>(),
        ["@gdp-ts/core.defineProof(\"NamespaceStarControl\")"]
    );
}

#[test]
fn gdp_producer_abstains_on_ambiguous_star_named_namespace_shapes() {
    let results = analyze_gdp_project(
        &[
            (
                "src/index.ts",
                "import { \"*\" as unrelated, defineProof } from './bridge'; unrelated.defineProof('LocalBareStarExport'); defineProof('BareStarControl');",
            ),
            (
                "src/other.ts",
                "const local = { defineProof: (kind) => kind }; export { local as \"*\" }; export { defineProof } from '@gdp-ts/core';",
            ),
            ("src/bridge.ts", "export * from './other';"),
        ],
        serde_json::json!([{"id":"trusted-producers","kind":"gdp-proof-producer","allowedFiles":["src/trusted/**"]}]),
    );
    assert_eq!(
        results
            .policy_violations
            .iter()
            .map(|finding| finding.violation.matched.as_str())
            .collect::<Vec<_>>(),
        ["@gdp-ts/core.defineProof(\"BareStarControl\")"]
    );
}

#[test]
fn gdp_producer_abstains_on_conflicting_explicit_and_duplicate_local_exports() {
    let results = analyze_gdp_project(
        &[
            (
                "src/index.ts",
                "import { defineProof as ambiguous } from './explicit'; import { ns } from './namespace'; import { defineProof } from '@gdp-ts/core'; import { defineProof as identical } from './identical-explicit'; import { ns as sameNs } from './identical-namespace'; ambiguous('ExplicitCollision'); ns.defineProof('NamespaceCollision'); defineProof('PositiveControl'); identical('IdenticalExplicitControl'); sameNs.defineProof('IdenticalNamespaceControl');",
            ),
            (
                "src/explicit.ts",
                "export { defineProof } from '@gdp-ts/core'; const unrelated = (kind) => kind; export { unrelated as defineProof };",
            ),
            (
                "src/namespace.ts",
                "import * as importedNs from '@gdp-ts/core'; const unrelated = { defineProof: (kind) => kind }; export { importedNs as ns, unrelated as ns };",
            ),
            (
                "src/identical-explicit.ts",
                "import { defineProof as imported } from '@gdp-ts/core'; export { defineProof } from '@gdp-ts/core'; export { imported as defineProof };",
            ),
            (
                "src/identical-namespace.ts",
                "import * as first from '@gdp-ts/core'; import * as second from '@gdp-ts/core'; export { first as ns, second as ns };",
            ),
        ],
        serde_json::json!([{"id":"trusted-producers","kind":"gdp-proof-producer","allowedFiles":["src/trusted/**"]}]),
    );
    assert_eq!(
        results
            .policy_violations
            .iter()
            .map(|finding| finding.violation.matched.as_str())
            .collect::<Vec<_>>(),
        [
            "@gdp-ts/core.defineProof(\"PositiveControl\")",
            "@gdp-ts/core.defineProof(\"IdenticalExplicitControl\")",
            "@gdp-ts/core.defineProof(\"IdenticalNamespaceControl\")"
        ]
    );
}

#[test]
fn gdp_producer_accepts_identical_imports_across_embedded_script_blocks() {
    let block = "import { defineProof } from '@gdp-ts/core';";
    let results = analyze_gdp_project(
        &[
            ("src/index.ts", "export {};"),
            (
                "src/Repeated.vue",
                &format!(
                    "<script lang=\"ts\">\n{block}\nexport const a = 1;\n</script>\n\n<script setup lang=\"ts\">\n{block}\nconst v = defineProof('VueRepeated');\n</script>\n"
                ),
            ),
            (
                "src/repeated.astro",
                &format!(
                    "---\n{block}\nconst v = defineProof('AstroRepeated');\n---\n<script>\n{block}\nconsole.log(defineProof);\n</script>\n"
                ),
            ),
            (
                "src/Conflicting.vue",
                "<script lang=\"ts\">\nimport { defineProof } from './local';\nexport const a = 1;\n</script>\n\n<script setup lang=\"ts\">\nimport { defineProof } from '@gdp-ts/core';\nconst v = defineProof('VueConflict');\n</script>\n",
            ),
            ("src/local.ts", "export const defineProof = (kind) => kind;"),
        ],
        serde_json::json!([{"id":"trusted-producers","kind":"gdp-proof-producer","allowedFiles":["src/trusted/**"]}]),
    );
    let mut matched: Vec<_> = results
        .policy_violations
        .iter()
        .map(|finding| finding.violation.matched.as_str())
        .collect();
    matched.sort_unstable();
    assert_eq!(
        matched,
        [
            "@gdp-ts/core.defineProof(\"AstroRepeated\")",
            "@gdp-ts/core.defineProof(\"VueRepeated\")"
        ]
    );
}
