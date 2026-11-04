use super::common::{create_config, fixture_path};

fn reported(config: &fallow_config::ResolvedConfig) -> Vec<(String, Vec<(String, u32)>)> {
    let results = fallow_core::analyze(config).expect("analysis should succeed");
    results
        .unlisted_binaries
        .iter()
        .map(|finding| {
            let sites = finding
                .binary
                .used_in
                .iter()
                .map(|site| {
                    let path = site
                        .path
                        .strip_prefix(&config.root)
                        .unwrap_or(&site.path)
                        .to_string_lossy()
                        .replace('\\', "/");
                    (path, site.line)
                })
                .collect();
            (finding.binary.binary_name.clone(), sites)
        })
        .collect()
}

fn sites(entries: &[(&str, u32)]) -> Vec<(String, u32)> {
    entries
        .iter()
        .map(|(path, line)| ((*path).to_string(), *line))
        .collect()
}

#[test]
fn reports_binaries_that_no_package_provides() {
    let mut config = create_config(fixture_path("unlisted-binaries"));
    config.ignore_binaries = fallow_config::IgnoreDependencyMatcher::compile(&["custom-*".into()]);

    assert_eq!(
        reported(&config),
        vec![
            (
                "gnome-extensions".to_string(),
                sites(&[("package.json", 7)])
            ),
            ("hyprctl".to_string(), sites(&[("src/index.ts", 3)])),
            ("tailscale".to_string(), sites(&[("src/index.ts", 4)])),
        ]
    );
}

#[test]
fn ignore_binaries_entries_and_the_rule_severity_apply() {
    let config = create_config(fixture_path("unlisted-binaries"));
    let names: Vec<String> = reported(&config)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert!(names.contains(&"custom-ignored".to_string()), "{names:?}");

    let mut off = create_config(fixture_path("unlisted-binaries"));
    off.rules.unlisted_binaries = fallow_config::Severity::Off;
    assert!(reported(&off).is_empty());
}
