//! napi-rs native package plugin.
//!
//! napi-rs packages publish platform-specific prebuilt binaries as optional
//! dependencies. Their generated loader selects the matching package at runtime,
//! so source imports do not show those packages as statically used.

use std::path::Path;

use fallow_config::{NapiConfig, PackageJson};
use rustc_hash::FxHashSet;

use super::Plugin;

pub struct NapiRsPlugin;

impl Plugin for NapiRsPlugin {
    fn name(&self) -> &'static str {
        "napi-rs"
    }

    fn is_enabled_with_package_json(&self, pkg: &PackageJson, _root: &Path) -> bool {
        pkg.napi.as_ref().is_some_and(|napi| {
            package_base(pkg, napi).is_some()
                && !napi.targets.is_empty()
                && pkg.optional_dependencies.is_some()
        })
    }

    fn package_json_referenced_dependencies(&self, pkg: &PackageJson, _root: &Path) -> Vec<String> {
        referenced_optional_dependencies(pkg)
    }
}

fn package_base<'a>(pkg: &'a PackageJson, napi: &'a NapiConfig) -> Option<&'a str> {
    napi.package_name
        .as_deref()
        .or(pkg.name.as_deref())
        .map(str::trim)
        .filter(|name| !name.is_empty())
}

fn referenced_optional_dependencies(pkg: &PackageJson) -> Vec<String> {
    let Some(napi) = &pkg.napi else {
        return Vec::new();
    };
    let Some(base) = package_base(pkg, napi) else {
        return Vec::new();
    };
    let optional_dependencies: FxHashSet<String> =
        pkg.optional_dependency_names().into_iter().collect();
    if optional_dependencies.is_empty() {
        return Vec::new();
    }

    let mut referenced: Vec<String> = napi
        .targets
        .iter()
        .filter_map(|target| target_suffix(target))
        .map(|suffix| format!("{base}-{suffix}"))
        .filter(|dependency| optional_dependencies.contains(dependency))
        .collect();
    referenced.sort();
    referenced.dedup();
    referenced
}

fn target_suffix(target: &str) -> Option<&'static str> {
    match target.trim() {
        "aarch64-apple-darwin" => Some("darwin-arm64"),
        "x86_64-apple-darwin" => Some("darwin-x64"),
        "aarch64-linux-android" => Some("android-arm64"),
        "armv7-linux-androideabi" => Some("android-arm-eabi"),
        "i686-linux-android" => Some("android-ia32"),
        "x86_64-linux-android" => Some("android-x64"),
        "aarch64-pc-windows-msvc" => Some("win32-arm64-msvc"),
        "i686-pc-windows-msvc" => Some("win32-ia32-msvc"),
        "x86_64-pc-windows-msvc" => Some("win32-x64-msvc"),
        "aarch64-unknown-freebsd" => Some("freebsd-arm64"),
        "x86_64-unknown-freebsd" => Some("freebsd-x64"),
        "aarch64-unknown-linux-gnu" => Some("linux-arm64-gnu"),
        "aarch64-unknown-linux-musl" => Some("linux-arm64-musl"),
        "armv7-unknown-linux-gnueabihf" => Some("linux-arm-gnueabihf"),
        "x86_64-unknown-linux-gnu" => Some("linux-x64-gnu"),
        "x86_64-unknown-linux-musl" => Some("linux-x64-musl"),
        "wasm32-wasi" | "wasm32-wasip1" => Some("wasm32-wasi"),
        "wasm32-wasi-preview1-threads" | "wasm32-wasip1-threads" => {
            Some("wasm32-wasi-singlethreaded")
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(raw: &str) -> PackageJson {
        serde_json::from_str(raw).expect("package json")
    }

    fn resolve(raw: &str) -> Vec<String> {
        referenced_optional_dependencies(&pkg(raw))
    }

    #[test]
    fn maps_common_targets_to_napi_package_suffixes() {
        for (target, suffix) in [
            ("aarch64-apple-darwin", "darwin-arm64"),
            ("x86_64-apple-darwin", "darwin-x64"),
            ("aarch64-unknown-linux-gnu", "linux-arm64-gnu"),
            ("aarch64-unknown-linux-musl", "linux-arm64-musl"),
            ("x86_64-unknown-linux-gnu", "linux-x64-gnu"),
            ("x86_64-unknown-linux-musl", "linux-x64-musl"),
            ("aarch64-pc-windows-msvc", "win32-arm64-msvc"),
            ("x86_64-pc-windows-msvc", "win32-x64-msvc"),
            ("wasm32-wasip1", "wasm32-wasi"),
            ("wasm32-wasip1-threads", "wasm32-wasi-singlethreaded"),
        ] {
            assert_eq!(target_suffix(target), Some(suffix));
        }
    }

    #[test]
    fn credits_scoped_optional_packages_from_package_name() {
        let referenced = resolve(
            r#"{
                "name": "@srcmap/codec",
                "optionalDependencies": {
                    "@srcmap/codec-darwin-arm64": "1.0.0",
                    "@srcmap/codec-linux-x64-gnu": "1.0.0",
                    "unused-optional-pkg": "1.0.0"
                },
                "napi": {
                    "binaryName": "srcmap-codec",
                    "targets": [
                        "aarch64-apple-darwin",
                        "x86_64-unknown-linux-gnu"
                    ]
                }
            }"#,
        );

        assert_eq!(
            referenced,
            vec![
                "@srcmap/codec-darwin-arm64".to_string(),
                "@srcmap/codec-linux-x64-gnu".to_string()
            ]
        );
    }

    #[test]
    fn package_name_overrides_manifest_name() {
        let referenced = resolve(
            r#"{
                "name": "development-helper",
                "optionalDependencies": {
                    "@oxc-coverage-instrument/binding-win32-arm64-msvc": "1.0.0",
                    "@oxc-coverage-instrument/binding-wasm32-wasi": "1.0.0",
                    "@oxc-coverage-instrument/binding-wasm32-wasi-singlethreaded": "1.0.0"
                },
                "napi": {
                    "packageName": "@oxc-coverage-instrument/binding",
                    "targets": [
                        "aarch64-pc-windows-msvc",
                        "wasm32-wasip1",
                        "wasm32-wasip1-threads"
                    ]
                }
            }"#,
        );

        assert_eq!(
            referenced,
            vec![
                "@oxc-coverage-instrument/binding-wasm32-wasi".to_string(),
                "@oxc-coverage-instrument/binding-wasm32-wasi-singlethreaded".to_string(),
                "@oxc-coverage-instrument/binding-win32-arm64-msvc".to_string()
            ]
        );
    }

    #[test]
    fn credits_unscoped_optional_packages() {
        let referenced = resolve(
            r#"{
                "name": "snappy",
                "optionalDependencies": {
                    "snappy-linux-x64-gnu": "1.0.0"
                },
                "napi": {
                    "targets": ["x86_64-unknown-linux-gnu"]
                }
            }"#,
        );

        assert_eq!(referenced, vec!["snappy-linux-x64-gnu"]);
    }

    #[test]
    fn does_not_credit_binary_name_as_package_base() {
        let referenced = resolve(
            r#"{
                "optionalDependencies": {
                    "srcmap-codec-darwin-arm64": "1.0.0"
                },
                "napi": {
                    "binaryName": "srcmap-codec",
                    "targets": ["aarch64-apple-darwin"]
                }
            }"#,
        );

        assert!(referenced.is_empty());
    }

    #[test]
    fn ignores_missing_optional_dependencies_and_unknown_targets() {
        let referenced = resolve(
            r#"{
                "name": "@scope/native",
                "optionalDependencies": {
                    "@scope/native-linux-x64-gnu": "1.0.0"
                },
                "napi": {
                    "targets": [
                        "x86_64-unknown-linux-gnu",
                        "unknown-target"
                    ]
                }
            }"#,
        );

        assert_eq!(referenced, vec!["@scope/native-linux-x64-gnu"]);
    }

    #[test]
    fn malformed_napi_config_is_ignored_by_package_parser() {
        let referenced = resolve(
            r#"{
                "name": "native",
                "optionalDependencies": {
                    "native-linux-x64-gnu": "1.0.0"
                },
                "napi": true
            }"#,
        );

        assert!(referenced.is_empty());
    }
}
