//! Hardhat Ethereum development plugin.
//!
//! Detects Hardhat projects and marks test, script, and deploy files as entry
//! points. Parses hardhat.config to extract plugin dependencies loaded via
//! both `import` statements and side-effect `require()` calls.

use super::config_parser;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["hardhat"];

const ENTRY_PATTERNS: &[&str] = &[
    "test/**/*.{ts,js}",
    "scripts/**/*.{ts,js}",
    "tasks/**/*.{ts,js}",
    "deploy/**/*.{ts,js}",
    "ignition/modules/**/*.{ts,js}",
];

const CONFIG_PATTERNS: &[&str] = &["hardhat.config.{ts,js,cjs,mjs}"];

const ALWAYS_USED: &[&str] = &["hardhat.config.{ts,js,cjs,mjs}"];

const TOOLING_DEPENDENCIES: &[&str] = &[
    "hardhat",
    "@nomicfoundation/hardhat-toolbox",
    "@nomicfoundation/hardhat-verify",
    "@nomicfoundation/hardhat-ethers",
    "@nomicfoundation/hardhat-chai-matchers",
    "@nomicfoundation/hardhat-network-helpers",
    "@nomicfoundation/hardhat-ignition",
    "@nomicfoundation/hardhat-ignition-ethers",
    "@nomiclabs/hardhat-waffle",
    "@nomiclabs/hardhat-ethers",
    "@nomiclabs/hardhat-etherscan",
    "@typechain/hardhat",
    "hardhat-gas-reporter",
    "hardhat-deploy",
    "hardhat-contract-sizer",
    "solidity-coverage",
    "solidity-docgen",
];

define_plugin! {
    struct HardhatPlugin => "hardhat",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    resolve_config(config_path, source, _root) {
        let mut result = PluginResult::default();

        let sources = config_parser::extract_imports_and_requires(source, config_path);
        for src in &sources {
            let dep = crate::resolve::extract_package_name(src);
            result.referenced_dependencies.push(dep);
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_config_extracts_plugin_imports() {
        let source = r#"
            import "@nomicfoundation/hardhat-toolbox";
            import "hardhat-gas-reporter";
            import { HardhatUserConfig } from "hardhat/config";
        "#;
        let plugin = HardhatPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("hardhat.config.ts"),
            source,
            std::path::Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"@nomicfoundation/hardhat-toolbox".to_string()));
        assert!(deps.contains(&"hardhat-gas-reporter".to_string()));
        assert!(deps.contains(&"hardhat".to_string()));
    }

    #[test]
    fn resolve_config_extracts_require_calls() {
        let source = r#"
            require("@nomiclabs/hardhat-waffle");
            require("@nomiclabs/hardhat-etherscan");
            module.exports = { solidity: "0.8.19" };
        "#;
        let plugin = HardhatPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("hardhat.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"@nomiclabs/hardhat-waffle".to_string()));
        assert!(deps.contains(&"@nomiclabs/hardhat-etherscan".to_string()));
    }

    #[test]
    fn resolve_config_mixed_imports_and_requires() {
        let source = r#"
            import "@nomicfoundation/hardhat-toolbox";
            require("hardhat-gas-reporter");
            export default { solidity: "0.8.19" };
        "#;
        let plugin = HardhatPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("hardhat.config.ts"),
            source,
            std::path::Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"@nomicfoundation/hardhat-toolbox".to_string()));
        assert!(deps.contains(&"hardhat-gas-reporter".to_string()));
    }

    #[test]
    fn resolve_config_empty() {
        let source = r#"module.exports = { solidity: "0.8.19" };"#;
        let plugin = HardhatPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("hardhat.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert_eq!(result.referenced_dependencies, [] as [String; 0]);
    }
}
