//! `fallow_api::run_combined` gives each section the production mode of its
//! `CombinedOptions::production_*` override. The overrides win over the
//! config and over `analysis.production_override`, also when a section
//! shares the dead-code session.

#![expect(
    clippy::expect_used,
    reason = "tests use expect to keep fixture setup concise"
)]

use fallow_api::{
    AnalysisOptions, CombinedOptions, ComplexityOptions, run_combined,
    serialize_combined_programmatic_json,
};

use crate::common::write;

/// A function in two test files: an unused file, a complexity finding and a
/// clone group that production mode removes.
const SENTINEL: &str = r"export function modeSentinel(value: number) {
  let result = value;
  if (value > 0) result += 1;
  if (value > 1) result += 2;
  if (value > 2) result += 3;
  if (value > 3) result += 4;
  return result;
}
";

#[test]
fn combined_sections_follow_the_production_overrides() {
    for mask in 0_u8..8 {
        let (dead_code, health, dupes) = (mask & 0b001 != 0, mask & 0b010 != 0, mask & 0b100 != 0);
        let project = tempfile::tempdir().expect("project");
        let root = project.path();
        write(
            root,
            "package.json",
            r#"{"name":"combined-overrides","type":"module","main":"src/index.ts"}"#,
        );
        write(
            root,
            ".fallowrc.json",
            r#"{"duplicates":{"minTokens":10,"minLines":2,"ignoreDefaults":false},"health":{"maxCyclomatic":2,"maxCognitive":2}}"#,
        );
        write(root, "src/index.ts", "export const entry = true;\n");
        write(root, "src/mode-sentinel.test.ts", SENTINEL);
        write(root, "src/mode-sentinel-copy.test.ts", SENTINEL);

        let output = run_combined(&CombinedOptions {
            analysis: AnalysisOptions {
                root: Some(root.to_path_buf()),
                no_cache: true,
                ..AnalysisOptions::default()
            },
            production_dead_code: Some(dead_code),
            production_health: Some(health),
            production_dupes: Some(dupes),
            include_entry_exports: true,
            health_options: ComplexityOptions {
                complexity: true,
                ..ComplexityOptions::default()
            },
            ..CombinedOptions::default()
        })
        .unwrap_or_else(|error| panic!("combined mask {mask:03b} failed: {error}"));
        let json = serialize_combined_programmatic_json(output)
            .unwrap_or_else(|error| panic!("serialize mask {mask:03b}: {error}"));

        for (section, production) in [("check", dead_code), ("health", health), ("dupes", dupes)] {
            let text = json[section].to_string();
            assert_eq!(
                text.contains("mode-sentinel.test.ts"),
                !production,
                "{section} scope mismatch for mask {mask:03b}: {text}"
            );
        }
    }
}
