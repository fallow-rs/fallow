//! React Native plugin.
//!
//! Detects React Native projects and marks app entry points and
//! Metro/Babel config files as always used.

use super::Plugin;

const ENABLERS: &[&str] = &["react-native"];

const ENTRY_PATTERNS: &[&str] = &[
    "index.{ts,tsx,js,jsx}",
    "App.{ts,tsx,js,jsx}",
    "src/App.{ts,tsx,js,jsx}",
    "app.config.{ts,js}",
];

const CONFIG_PATTERNS: &[&str] = &["metro.config.{ts,js}", "react-native.config.{ts,js}"];

const ALWAYS_USED: &[&str] = &[
    "metro.config.{ts,js}",
    "react-native.config.{ts,js}",
    "babel.config.{ts,js}",
    "app.json",
];

const TOOLING_DEPENDENCIES: &[&str] = &[
    "react-native",
    "metro",
    "metro-config",
    "@react-native-community/cli",
    "@react-native/metro-config",
];

define_plugin! {
    struct ReactNativePlugin => "react-native",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
}
