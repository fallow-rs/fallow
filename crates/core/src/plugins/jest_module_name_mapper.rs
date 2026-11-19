//! Read the `moduleNameMapper` targets of a Jest config.

use std::path::Path;

use super::config_parser;

/// The `moduleNameMapper` targets that Jest loads, split into the files under
/// `<rootDir>` and the packages.
pub(super) struct ModuleNameMapperTargets {
    pub(super) files: Vec<String>,
    pub(super) packages: Vec<String>,
}

/// Collect the `moduleNameMapper` targets that name one file under
/// `<rootDir>` or one package.
///
/// Jest resolves a relative target from the directory of each importing file,
/// so only the token form names a known file. A file target with a `$1`
/// capture group names many files. A bare target names a package, and a
/// capture group after the package name still names that one package.
pub(super) fn targets(parse_source: &str, parse_path: &Path) -> ModuleNameMapperTargets {
    let targets = config_parser::extract_config_property_strings(
        parse_source,
        parse_path,
        "moduleNameMapper",
    );
    let mut mapped = ModuleNameMapperTargets {
        files: Vec::new(),
        packages: Vec::new(),
    };
    for target in targets {
        if target.starts_with("<rootDir>") {
            if !target.contains('$') {
                mapped.files.push(target);
            }
            continue;
        }
        if !config_parser::is_package_specifier(&target) {
            continue;
        }
        let package = crate::resolve::extract_package_name(&target);
        if !package.contains('$') {
            mapped.packages.push(package);
        }
    }
    mapped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_targets_name_packages() {
        let source = r#"
            module.exports = {
                moduleNameMapper: {
                    "\\.(css|less)$": "identity-obj-proxy",
                    "^lodash/(.*)$": "lodash-es/$1",
                    "^@ui/(.*)$": "@scope/ui/dist/$1",
                    "^~/(.*)$": "$1",
                    "^inquirer$": "<rootDir>/mocks/inquirer.js",
                    "^@/(.*)$": "<rootDir>/src/$1",
                    "^local$": "./local.js"
                }
            };
        "#;
        let mut targets = targets(source, Path::new("jest.config.js"));
        targets.packages.sort();
        assert_eq!(
            targets.packages,
            vec!["@scope/ui", "identity-obj-proxy", "lodash-es"]
        );
        assert_eq!(targets.files, vec!["<rootDir>/mocks/inquirer.js"]);
    }
}
