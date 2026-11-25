//! `@vite-pwa/assets-generator` plugin.
//!
//! The generator CLI and the `pwaAssets` option of `vite-plugin-pwa` load
//! `pwa-assets.config.*` from the working directory, so no module imports it.
//! The patterns are anchored to the project root and to each workspace
//! package root: a file of the same name in a nested directory is not a
//! config of the tool.

use super::Plugin;

const ENABLERS: &[&str] = &["@vite-pwa/assets-generator"];

const ALWAYS_USED: &[&str] = &["pwa-assets.config.{ts,mts,cts,js,mjs,cjs,json}"];

const CONFIG_FILE_EXPORTS: &[(&str, &[&str])] =
    &[("pwa-assets.config.{ts,mts,cts,js,mjs,cjs}", &["default"])];

const TOOLING_DEPENDENCIES: &[&str] = &["@vite-pwa/assets-generator"];

define_plugin! {
    struct VitePwaAssetsGeneratorPlugin => "vite-pwa-assets-generator",
    enablers: ENABLERS,
    config_file_exports: CONFIG_FILE_EXPORTS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
}
