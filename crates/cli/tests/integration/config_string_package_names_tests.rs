//! A package that a config file names only as a string, such as an SWC
//! plugin, a Babel preset in a plugin option, or a Nest schematics
//! collection, is credited by the unused-dependency check. A listed package
//! that no file names stays reported.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use crate::common::{parse_json, run_fallow};

const FIXTURE: &str = "config-string-package-names";

fn unused_dev_dependencies() -> Vec<String> {
    let output = parse_json(&run_fallow(
        "dead-code",
        FIXTURE,
        &["--format", "json", "--quiet", "--no-cache"],
    ));
    let mut unused: Vec<String> = output["unused_dev_dependencies"]
        .as_array()
        .expect("unused_dev_dependencies array")
        .iter()
        .map(|dep| {
            dep["package_name"]
                .as_str()
                .expect("package_name")
                .to_string()
        })
        .collect();
    unused.sort_unstable();
    unused
}

#[test]
fn packages_named_in_config_strings_are_not_reported_as_unused() {
    assert_eq!(unused_dev_dependencies(), vec!["unused-dev-tool"]);
}
