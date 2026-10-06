//! nodemon plugin.
//!
//! Detects nodemon projects and marks config files as always used. nodemon
//! also reads its config from the package.json `nodemonConfig` key.

use super::Plugin;

const ENABLERS: &[&str] = &["nodemon"];

const ALWAYS_USED: &[&str] = &["nodemon.json", ".nodemonrc", ".nodemonrc.{json,yml,yaml}"];

const TOOLING_DEPENDENCIES: &[&str] = &["nodemon"];

define_plugin! {
    struct NodemonPlugin => "nodemon",
    enablers: ENABLERS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    package_json_config_key: "nodemonConfig",
}
