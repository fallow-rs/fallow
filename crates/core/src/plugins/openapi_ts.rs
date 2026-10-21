//! openapi-ts plugin.
//!
//! Detects openapi-ts projects and marks config files as always used.

use super::Plugin;

const ENABLERS: &[&str] = &["openapi-typescript"];

const ALWAYS_USED: &[&str] = &["openapi-ts.config.{ts,js,cjs,mjs}"];

const CONFIG_FILE_EXPORTS: &[(&str, &[&str])] =
    &[("openapi-ts.config.{ts,js,cjs,mjs}", &["default"])];

const TOOLING_DEPENDENCIES: &[&str] = &["openapi-typescript", "openapi-typescript-codegen"];

define_plugin! {
    struct OpenapiTsPlugin => "openapi-ts",
    enablers: ENABLERS,
    config_file_exports: CONFIG_FILE_EXPORTS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
}
