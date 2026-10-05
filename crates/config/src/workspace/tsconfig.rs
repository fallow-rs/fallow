use std::path::{Component, Path, PathBuf};

use rustc_hash::FxHashSet;

const MAX_EXTENDS_DEPTH: usize = 16;
const DECLARATION_OUTPUT_SUFFIXES: &[&str] = &[".d.ts", ".d.mts", ".d.cts"];

#[derive(Debug, Clone, Default)]
struct CompilerOptions {
    root_dir: Option<PathBuf>,
    out_dir: Option<PathBuf>,
    declaration_dir: Option<PathBuf>,
    no_emit: Option<bool>,
    emit_declaration_only: Option<bool>,
}

/// Outcome for a package entry that may point into a configured TypeScript output directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TsconfigOutputResolution {
    /// No supported config declares this entry as an output.
    Unconfigured,
    /// A config declares the output path, but no unique source mapping is available.
    ConfiguredButUnresolved,
    /// The configured output maps to one source file inside the project.
    Resolved(PathBuf),
}

/// Static package output mappings read from a package's TypeScript configs.
#[derive(Debug, Clone, Default)]
pub struct TsconfigOutputMap {
    root: Option<PathBuf>,
    display_root: Option<PathBuf>,
    configs: Vec<CompilerOptions>,
}

impl TsconfigOutputMap {
    /// Read root-level tsconfig files once, resolving static relative and package `extends` paths.
    #[must_use]
    pub fn from_project(root: &Path) -> Self {
        let display_root = root.to_path_buf();
        let Some(canonical_root) = dunce::canonicalize(root).ok() else {
            return Self::default();
        };
        let Some(config_paths) = tsconfig_paths(&canonical_root) else {
            return Self::default();
        };
        let configs = config_paths
            .iter()
            .filter_map(|path| {
                let config_dir = path.parent()?;
                read_compiler_options(path, config_dir, &mut FxHashSet::default(), 0)
            })
            .collect();
        Self {
            root: Some(canonical_root),
            display_root: Some(display_root),
            configs,
        }
    }

    /// Resolve an absolute output path, such as the target of a relative import,
    /// to its source file.
    ///
    /// The path does not need to exist on disk. A path outside the project root
    /// of this map is [`TsconfigOutputResolution::Unconfigured`].
    #[must_use]
    pub fn resolve_source_for_output_path(
        &self,
        output_path: &Path,
        source_extensions: &[&str],
    ) -> TsconfigOutputResolution {
        let Some(output_path) = normalize_path(output_path) else {
            return TsconfigOutputResolution::Unconfigured;
        };
        let relative = [self.display_root.as_deref(), self.root.as_deref()]
            .into_iter()
            .flatten()
            .find_map(|root| output_path.strip_prefix(root).ok());
        let Some(entry) = relative.and_then(Path::to_str) else {
            return TsconfigOutputResolution::Unconfigured;
        };
        self.resolve_source_for_entry(entry, source_extensions)
    }

    /// Resolve a generated entry while preserving whether the output was configured.
    #[must_use]
    pub fn resolve_source_for_entry(
        &self,
        entry: &str,
        source_extensions: &[&str],
    ) -> TsconfigOutputResolution {
        let Some(root) = self.root.as_deref() else {
            return TsconfigOutputResolution::Unconfigured;
        };
        let entry_path = Path::new(entry);
        if entry_path.is_absolute()
            || entry_path
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            return TsconfigOutputResolution::Unconfigured;
        }
        let Some(entry_path) = normalize_path(&root.join(entry_path)) else {
            return TsconfigOutputResolution::Unconfigured;
        };
        let mut candidates = FxHashSet::default();
        let mut configured = false;

        for options in &self.configs {
            let Some(file_name) = entry_path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let is_declaration = DECLARATION_OUTPUT_SUFFIXES
                .iter()
                .any(|suffix| file_name.ends_with(suffix));
            if options.emit_declaration_only.unwrap_or(false) && !is_declaration {
                continue;
            }
            let output_dir = if is_declaration {
                options
                    .declaration_dir
                    .as_deref()
                    .or(options.out_dir.as_deref())
            } else {
                options.out_dir.as_deref()
            };
            let Some(output_dir) = output_dir else {
                continue;
            };
            let Some((source_stem, family_extensions)) =
                source_suffix_for_output(&entry_path, output_dir)
            else {
                continue;
            };
            let Some(root_dir) = options.root_dir.as_deref() else {
                continue;
            };
            configured = true;
            if options.no_emit.unwrap_or(false) {
                continue;
            }
            for extension in family_extensions {
                if !source_extensions.contains(extension) {
                    continue;
                }
                let Some(candidate) = append_extension(&source_stem, extension) else {
                    continue;
                };
                let candidate = root_dir.join(candidate);
                let Ok(candidate) = dunce::canonicalize(candidate) else {
                    continue;
                };
                if candidate.starts_with(root) && candidate.is_file() {
                    candidates.insert(candidate);
                }
            }
        }

        if candidates.len() != 1 {
            return if configured {
                TsconfigOutputResolution::ConfiguredButUnresolved
            } else {
                TsconfigOutputResolution::Unconfigured
            };
        }
        let Some(source) = candidates.into_iter().next() else {
            return TsconfigOutputResolution::ConfiguredButUnresolved;
        };
        let Some(relative) = source.strip_prefix(root).ok() else {
            return TsconfigOutputResolution::ConfiguredButUnresolved;
        };
        let Some(display_root) = self.display_root.as_deref() else {
            return TsconfigOutputResolution::ConfiguredButUnresolved;
        };
        TsconfigOutputResolution::Resolved(display_root.join(relative))
    }
}

fn tsconfig_paths(root: &Path) -> Option<Vec<PathBuf>> {
    let mut configs = std::fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("tsconfig"))
                && path.extension().is_some_and(|extension| {
                    extension.eq_ignore_ascii_case("json")
                        || extension.eq_ignore_ascii_case("jsonc")
                })
        })
        .collect::<Vec<_>>();
    configs.sort();
    Some(configs)
}

fn read_compiler_options(
    config_path: &Path,
    substitution_base: &Path,
    visited: &mut FxHashSet<PathBuf>,
    depth: usize,
) -> Option<CompilerOptions> {
    if depth >= MAX_EXTENDS_DEPTH {
        return None;
    }
    let config_path = dunce::canonicalize(config_path).ok()?;
    if !visited.insert(config_path.clone()) {
        return None;
    }
    let content = std::fs::read_to_string(&config_path).ok()?;
    let value: serde_json::Value =
        crate::jsonc::parse_to_value(content.trim_start_matches('\u{FEFF}')).ok()?;
    let parent_options = match value.get("extends") {
        Some(serde_json::Value::String(extends)) => {
            let parent = resolve_extends_path(&config_path, extends)?;
            read_compiler_options(&parent, substitution_base, visited, depth + 1)?
        }
        Some(_) => return None,
        None => CompilerOptions::default(),
    };
    let mut options = parent_options;
    let Some(compiler_options) = value.get("compilerOptions") else {
        visited.remove(&config_path);
        return Some(options);
    };
    let compiler_options = compiler_options.as_object()?;
    let config_dir = config_path.parent()?;

    replace_path_option(
        compiler_options,
        "rootDir",
        config_dir,
        substitution_base,
        &mut options.root_dir,
    )?;
    replace_path_option(
        compiler_options,
        "outDir",
        config_dir,
        substitution_base,
        &mut options.out_dir,
    )?;
    replace_path_option(
        compiler_options,
        "declarationDir",
        config_dir,
        substitution_base,
        &mut options.declaration_dir,
    )?;
    replace_bool_option(compiler_options, "noEmit", &mut options.no_emit)?;
    replace_bool_option(
        compiler_options,
        "emitDeclarationOnly",
        &mut options.emit_declaration_only,
    )?;
    visited.remove(&config_path);
    Some(options)
}

fn replace_path_option(
    compiler_options: &serde_json::Map<String, serde_json::Value>,
    name: &str,
    config_dir: &Path,
    substitution_base: &Path,
    option: &mut Option<PathBuf>,
) -> Option<()> {
    if let Some(value) = compiler_options.get(name) {
        let value = value.as_str()?;
        if value.is_empty() {
            return None;
        }
        let value = value.replace("${configDir}", &substitution_base.to_string_lossy());
        let path = Path::new(&value);
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            config_dir.join(path)
        };
        *option = Some(normalize_path(&path)?);
    }
    Some(())
}

fn replace_bool_option(
    compiler_options: &serde_json::Map<String, serde_json::Value>,
    name: &str,
    option: &mut Option<bool>,
) -> Option<()> {
    if let Some(value) = compiler_options.get(name) {
        *option = Some(value.as_bool()?);
    }
    Some(())
}

/// Find the configs in the `extends` chain of `config_path` that exist but fail
/// to parse. `value` is the already parsed content of `config_path`.
///
/// Returns `(path, parser message)` pairs in walk order. A target that does not
/// resolve to a file is skipped, because this walk reports syntax only. The
/// walk follows string and array `extends` with the same lookup that
/// [`TsconfigOutputMap`] uses, and stops at [`MAX_EXTENDS_DEPTH`].
pub(super) fn malformed_extends_parents(
    config_path: &Path,
    value: &serde_json::Value,
) -> Vec<(PathBuf, String)> {
    let mut malformed = Vec::new();
    let mut visited = FxHashSet::default();
    visited.insert(config_path.to_path_buf());
    let mut frontier = extends_parents(config_path, value)
        .into_iter()
        .map(|parent| (parent, 1))
        .collect::<Vec<_>>();
    while let Some((path, depth)) = frontier.pop() {
        if depth > MAX_EXTENDS_DEPTH || !visited.insert(path.clone()) {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        match crate::jsonc::parse_to_value::<serde_json::Value>(
            content.trim_start_matches('\u{FEFF}'),
        ) {
            Ok(parent_value) => frontier.extend(
                extends_parents(&path, &parent_value)
                    .into_iter()
                    .map(|parent| (parent, depth + 1)),
            ),
            Err(error) => malformed.push((path, error.to_string())),
        }
    }
    malformed
}

/// The resolved parent configs of one tsconfig: `extends` is one string or,
/// since TypeScript 5.0, an array of strings.
fn extends_parents(config_path: &Path, value: &serde_json::Value) -> Vec<PathBuf> {
    let targets: Vec<&str> = match value.get("extends") {
        Some(serde_json::Value::String(target)) => vec![target.as_str()],
        Some(serde_json::Value::Array(items)) => {
            items.iter().filter_map(serde_json::Value::as_str).collect()
        }
        _ => Vec::new(),
    };
    targets
        .into_iter()
        .filter_map(|target| resolve_extends_path(config_path, target))
        .collect()
}

fn resolve_extends_path(config_path: &Path, extends: &str) -> Option<PathBuf> {
    let path = Path::new(extends);
    let config_dir = config_path.parent()?;
    let explicitly_local =
        path.is_absolute() || extends.starts_with("./") || extends.starts_with("../");
    if !explicitly_local {
        return resolve_package_extends(config_dir, extends);
    }
    let path = normalize_path(&config_dir.join(path))?;
    if path.is_file() {
        return Some(path);
    }
    let directory_config = path.join("tsconfig.json");
    if directory_config.is_file() {
        return Some(directory_config);
    }
    let file_name = path.file_name()?.to_string_lossy();
    if !file_name.ends_with(".json") && !file_name.ends_with(".jsonc") {
        let json = append_path_suffix(&path, ".json")?;
        if json.is_file() {
            return Some(json);
        }
        let jsonc = append_path_suffix(&path, ".jsonc")?;
        if jsonc.is_file() {
            return Some(jsonc);
        }
    }
    None
}

fn resolve_package_extends(config_dir: &Path, extends: &str) -> Option<PathBuf> {
    let (package_name, subpath) = package_extends_parts(extends)?;
    let mut directory = config_dir.to_path_buf();

    for _ in 0..MAX_EXTENDS_DEPTH {
        let package_root = directory.join("node_modules").join(&package_name);
        if package_root.is_dir() {
            if let Some(subpath) = subpath.as_deref() {
                return package_subpath_tsconfig(&package_root, subpath);
            }
            return package_root_tsconfig(&package_root);
        }
        if !directory.pop() {
            break;
        }
    }
    None
}

fn package_subpath_tsconfig(package_root: &Path, subpath: &str) -> Option<PathBuf> {
    let package_json_path = package_root.join("package.json");
    if package_json_path.is_file() {
        let package_json = std::fs::read_to_string(package_json_path).ok()?;
        let package_json: serde_json::Value = serde_json::from_str(&package_json).ok()?;
        if let Some(exports) = package_json.get("exports") {
            let serde_json::Value::Object(exports) = exports else {
                return None;
            };
            let key = format!("./{subpath}");
            let target = exports.get(&key)?.as_str()?;
            return resolve_exported_config_file(package_root, target);
        }
    }
    resolve_config_file(package_root, &format!("./{subpath}"))
}

fn package_extends_parts(extends: &str) -> Option<(PathBuf, Option<String>)> {
    let parts = extends.split('/').collect::<Vec<_>>();
    let package_parts = if parts.first()?.starts_with('@') {
        2
    } else {
        1
    };
    if parts.len() < package_parts || parts.iter().any(|part| part.is_empty() || *part == "..") {
        return None;
    }
    let package = parts[..package_parts].join("/");
    let subpath = (parts.len() > package_parts).then(|| parts[package_parts..].join("/"));
    Some((PathBuf::from(package), subpath))
}

fn package_root_tsconfig(package_root: &Path) -> Option<PathBuf> {
    let package_json_path = package_root.join("package.json");
    if package_json_path.is_file() {
        let package_json = std::fs::read_to_string(package_json_path).ok()?;
        let package_json: serde_json::Value = serde_json::from_str(&package_json).ok()?;
        if let Some(exports) = package_json.get("exports") {
            let target = match exports {
                serde_json::Value::String(target) => Some(target.as_str()),
                serde_json::Value::Object(map) => map.get(".").and_then(serde_json::Value::as_str),
                _ => None,
            }?;
            return resolve_exported_config_file(package_root, target);
        }
        if let Some(tsconfig) = package_json.get("tsconfig") {
            return resolve_package_tsconfig_field(package_root, tsconfig.as_str()?);
        }
    }
    let conventional = package_root.join("tsconfig.json");
    conventional.is_file().then_some(conventional)
}

fn resolve_package_tsconfig_field(package_root: &Path, target: &str) -> Option<PathBuf> {
    if target.starts_with("./") {
        return resolve_config_file(package_root, target);
    }
    if target.is_empty()
        || target.starts_with('/')
        || target.contains('*')
        || Path::new(target)
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return None;
    }
    resolve_config_file(package_root, &format!("./{target}"))
}

fn resolve_exported_config_file(root: &Path, target: &str) -> Option<PathBuf> {
    let path = resolve_config_target(root, target)?;
    (path.extension()? == "json" && path.is_file()).then_some(path)
}

fn resolve_config_file(root: &Path, target: &str) -> Option<PathBuf> {
    let path = resolve_config_target(root, target)?;
    if path
        .extension()
        .is_some_and(|extension| extension == "json")
        && path.is_file()
    {
        return Some(path);
    }
    let candidate = append_path_suffix(&path, ".json")?;
    candidate.is_file().then_some(candidate)
}

fn resolve_config_target(root: &Path, target: &str) -> Option<PathBuf> {
    if !target.starts_with("./") || target.contains('*') {
        return None;
    }
    let relative = Path::new(target);
    relative.file_name()?;
    if relative
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return None;
    }
    normalize_path(&root.join(relative))
}

fn source_suffix_for_output(
    entry: &Path,
    out_dir: &Path,
) -> Option<(PathBuf, &'static [&'static str])> {
    let relative = entry.strip_prefix(out_dir).ok()?;
    let file_name = relative.file_name()?.to_str()?;
    output_source_stem(relative, file_name)
}

fn output_source_stem(path: &Path, file_name: &str) -> Option<(PathBuf, &'static [&'static str])> {
    if let Some(suffix) = DECLARATION_OUTPUT_SUFFIXES
        .iter()
        .find(|suffix| file_name.ends_with(**suffix))
    {
        let stem = file_name.strip_suffix(suffix)?;
        let extensions = match *suffix {
            ".d.ts" => &["ts", "tsx", "js", "jsx"][..],
            ".d.mts" => &["mts", "mjs"][..],
            ".d.cts" => &["cts", "cjs"][..],
            _ => return None,
        };
        return Some((path.parent()?.join(stem), extensions));
    }

    let extension = path.extension()?.to_str()?;
    let stem = file_name.strip_suffix(&format!(".{extension}"))?;
    let extensions = match extension {
        "js" => &["ts", "tsx", "js", "jsx"][..],
        "jsx" => &["tsx", "jsx"][..],
        "mjs" => &["mts", "mjs"][..],
        "cjs" => &["cts", "cjs"][..],
        _ => return None,
    };
    Some((path.parent()?.join(stem), extensions))
}

fn append_extension(stem: &Path, extension: &str) -> Option<PathBuf> {
    let file_name = stem.file_name()?.to_str()?;
    Some(stem.parent()?.join(format!("{file_name}.{extension}")))
}

fn append_path_suffix(path: &Path, suffix: &str) -> Option<PathBuf> {
    let file_name = path.file_name()?.to_str()?;
    Some(path.parent()?.join(format!("{file_name}{suffix}")))
}

fn normalize_path(path: &Path) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    Some(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().expect("parent directory")).expect("create parent");
        std::fs::write(path, content).expect("write file");
    }

    fn map(root: &Path, entry: &str, source_extensions: &[&str]) -> TsconfigOutputResolution {
        TsconfigOutputMap::from_project(root).resolve_source_for_entry(entry, source_extensions)
    }

    #[test]
    fn maps_generated_entry_with_inherited_paths_relative_to_their_config_file() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(
            &root.join("configs/tsconfig.base.json"),
            r#"{
                "compilerOptions": {
                    "rootDir": "../source",
                    "outDir": "../distribution"
                }
            }"#,
        );
        write(
            &root.join("tsconfig.build.json"),
            r#"{"extends":"./configs/tsconfig.base.json"}"#,
        );
        write(&root.join("source/index.ts"), "export const value = 1;\n");

        assert_eq!(
            map(root, "./distribution/index.js", &["ts", "tsx", "js"]),
            TsconfigOutputResolution::Resolved(root.join("source/index.ts"))
        );
    }

    #[test]
    fn no_emit_configs_do_not_map_public_outputs() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(
            &root.join("configs/tsconfig.base.json"),
            r#"{
                "compilerOptions": {
                    "rootDir": "../source",
                    "outDir": "../distribution"
                }
            }"#,
        );
        write(
            &root.join("tsconfig.test.json"),
            r#"{
                "extends": "./configs/tsconfig.base.json",
                "compilerOptions": {"noEmit": true}
            }"#,
        );
        write(&root.join("source/index.ts"), "export const value = 1;\n");

        assert_eq!(
            map(root, "./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::ConfiguredButUnresolved
        );
        write(
            &root.join("tsconfig.test.json"),
            r#"{"extends":"./configs/tsconfig.base.json","compilerOptions":{"noEmit":false}}"#,
        );
        assert_eq!(
            map(root, "./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("source/index.ts"))
        );
    }

    #[test]
    fn ambiguous_source_candidates_fail_closed() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(
            &root.join("tsconfig.build.json"),
            r#"{
                "compilerOptions": {
                    "rootDir": "./source",
                    "outDir": "./distribution"
                }
            }"#,
        );
        write(&root.join("source/index.ts"), "export const value = 1;\n");
        write(&root.join("source/index.js"), "export const value = 1;\n");

        assert_eq!(
            map(root, "./distribution/index.js", &["ts", "js"]),
            TsconfigOutputResolution::ConfiguredButUnresolved
        );
        assert_eq!(
            map(root, "./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("source/index.ts"))
        );
    }

    #[test]
    fn maps_dotted_basenames_and_keeps_module_output_families_separate() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(
            &root.join("tsconfig.build.json"),
            r#"{
                "compilerOptions": {
                    "rootDir": "./source",
                    "outDir": "./distribution"
                }
            }"#,
        );
        write(
            &root.join("source/feature.test.ts"),
            "export const value = 1;\n",
        );
        write(
            &root.join("source/feature.native.mts"),
            "export const native = 1;\n",
        );

        assert_eq!(
            map(root, "./distribution/feature.test.js", &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("source/feature.test.ts"))
        );
        assert_eq!(
            map(root, "./distribution/feature.test.mjs", &["ts"]),
            TsconfigOutputResolution::ConfiguredButUnresolved,
            "an MJS output must not resolve to an unrelated TS source with the same basename"
        );
        assert_eq!(
            map(root, "./distribution/feature.native.mjs", &["mts"]),
            TsconfigOutputResolution::Resolved(root.join("source/feature.native.mts"))
        );
    }

    #[test]
    fn resolves_installed_package_extends_and_config_dir_options_statically() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(
            &root.join("node_modules/@scope/config/package.json"),
            r#"{"exports":"./shared.json"}"#,
        );
        write(
            &root.join("node_modules/@scope/config/shared.json"),
            r#"{
                "compilerOptions": {
                    "outDir": "${configDir}/distribution"
                }
            }"#,
        );
        write(
            &root.join("tsconfig.json"),
            r#"{"extends":"@scope/config"}"#,
        );
        write(
            &root.join("tsconfig.build.json"),
            r#"{
                "extends":"./tsconfig.json",
                "compilerOptions":{"rootDir":"./source"}
            }"#,
        );
        write(&root.join("source/index.ts"), "export const value = 1;\n");

        assert_eq!(
            map(root, "./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("source/index.ts"))
        );
    }

    #[test]
    fn bare_extends_uses_package_lookup_and_dotted_local_stems_append_json() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(
            &root.join("configs/tsconfig.base.json"),
            r#"{
                "compilerOptions": {
                    "rootDir": "../source",
                    "outDir": "../local-output"
                }
            }"#,
        );
        write(
            &root.join("node_modules/shared-config/package.json"),
            r#"{"exports":"./shared.json"}"#,
        );
        write(
            &root.join("node_modules/shared-config/shared.json"),
            r#"{"compilerOptions":{"outDir":"../../distribution"}}"#,
        );
        write(
            &root.join("shared-config"),
            r#"{"compilerOptions":{"outDir":"./wrong-output"}}"#,
        );
        write(
            &root.join("tsconfig.local.json"),
            r#"{"extends":"./configs/tsconfig.base"}"#,
        );
        write(
            &root.join("tsconfig.package.json"),
            r#"{
                "extends":"shared-config",
                "compilerOptions":{"rootDir":"./source"}
            }"#,
        );
        write(&root.join("source/index.ts"), "export const value = 1;\n");

        let map = TsconfigOutputMap::from_project(root);
        assert_eq!(
            map.resolve_source_for_entry("./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("source/index.ts")),
            "bare package names must resolve through node_modules even with a same-named local file"
        );
        assert_eq!(
            map.resolve_source_for_entry("./local-output/index.js", &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("source/index.ts")),
            "dotted local config stems should resolve by appending .json"
        );
    }

    #[test]
    fn package_config_fields_and_unexported_subpaths_resolve_json_files() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        let package_root = root.join("node_modules/shared-config");
        for file in [
            "valid.json",
            "dotted.base.json",
            "double.json.json",
            "extensionless",
            "invalid.jsonc",
            "invalid.ts",
        ] {
            write(
                &package_root.join(file),
                r#"{"compilerOptions":{"outDir":"../../distribution"}}"#,
            );
        }
        write(&root.join("source/index.ts"), "export const value = 1;\n");

        for use_field in [true, false] {
            for (target, resolves) in [
                ("valid.json", true),
                ("valid", true),
                ("dotted.base", true),
                ("double.json", true),
                ("extensionless", false),
                ("invalid.jsonc", false),
                ("invalid.ts", false),
            ] {
                let (manifest, extends) = if use_field {
                    (
                        serde_json::json!({"tsconfig": target}),
                        "shared-config".to_string(),
                    )
                } else {
                    (serde_json::json!({}), format!("shared-config/{target}"))
                };
                write(&package_root.join("package.json"), &manifest.to_string());
                write(
                    &root.join("tsconfig.json"),
                    &serde_json::json!({
                        "extends": extends,
                        "compilerOptions": {"rootDir": "./source"}
                    })
                    .to_string(),
                );
                let expected = if resolves {
                    TsconfigOutputResolution::Resolved(root.join("source/index.ts"))
                } else {
                    TsconfigOutputResolution::Unconfigured
                };
                assert_eq!(
                    map(root, "./distribution/index.js", &["ts"]),
                    expected,
                    "package config target {target}, tsconfig field present={use_field}"
                );
            }
        }
    }

    #[test]
    fn package_exports_precede_conventional_tsconfig_and_conditions_fail_closed() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(
            &root.join("node_modules/shared-config/package.json"),
            r#"{"exports":{".":"./build.json"}}"#,
        );
        write(
            &root.join("node_modules/shared-config/build.json"),
            r#"{"compilerOptions":{"outDir":"../../distribution"}}"#,
        );
        write(
            &root.join("node_modules/shared-config/tsconfig.json"),
            r#"{"compilerOptions":{"outDir":"../../other"}}"#,
        );
        write(
            &root.join("node_modules/conditional-config/package.json"),
            r#"{"exports":{".":{"types":"./types.json"}},"tsconfig":"./fallback.json"}"#,
        );
        write(
            &root.join("node_modules/conditional-config/types.json"),
            r#"{"compilerOptions":{"outDir":"../../conditional-output"}}"#,
        );
        write(
            &root.join("node_modules/conditional-config/fallback.json"),
            r#"{"compilerOptions":{"outDir":"../../conditional-output"}}"#,
        );
        write(
            &root.join("tsconfig.build.json"),
            r#"{
                "extends":"shared-config",
                "compilerOptions":{"rootDir":"./source"}
            }"#,
        );
        write(
            &root.join("tsconfig.conditional.json"),
            r#"{
                "extends":"conditional-config",
                "compilerOptions":{"rootDir":"./source"}
            }"#,
        );
        write(&root.join("source/index.ts"), "export const value = 1;\n");

        let map = TsconfigOutputMap::from_project(root);
        assert_eq!(
            map.resolve_source_for_entry("./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("source/index.ts")),
            "the simple root export should be selected before conventional tsconfig.json"
        );
        assert_eq!(
            map.resolve_source_for_entry("./other/index.js", &["ts"]),
            TsconfigOutputResolution::Unconfigured,
            "a simple root export must take precedence over the conventional config"
        );
        assert_eq!(
            map.resolve_source_for_entry("./conditional-output/index.js", &["ts"]),
            TsconfigOutputResolution::Unconfigured,
            "unsupported conditional package exports must not fall through to package tsconfig"
        );
    }

    #[test]
    fn package_export_targets_require_explicit_config_extensions() {
        for (extends, export_key) in [
            ("shared-config", None),
            ("shared-config", Some(".")),
            ("shared-config/build", Some("./build")),
        ] {
            let directory = tempfile::tempdir().expect("temporary directory");
            let root = directory.path();
            let package_root = root.join("node_modules/shared-config");
            for file in [
                "build.json",
                "build",
                "build.jsonc",
                "build.ts",
                "inferred.json",
                "double.json.json",
            ] {
                write(
                    &package_root.join(file),
                    r#"{"compilerOptions":{"outDir":"../../distribution"}}"#,
                );
            }
            write(
                &root.join("tsconfig.json"),
                &format!(r#"{{"extends":"{extends}","compilerOptions":{{"rootDir":"./source"}}}}"#),
            );
            write(&root.join("source/index.ts"), "export const value = 1;\n");

            for (target, expected) in [
                ("./build", TsconfigOutputResolution::Unconfigured),
                ("./build.jsonc", TsconfigOutputResolution::Unconfigured),
                ("./build.ts", TsconfigOutputResolution::Unconfigured),
                ("./inferred", TsconfigOutputResolution::Unconfigured),
                ("./double.json", TsconfigOutputResolution::Unconfigured),
                (
                    "./build.json",
                    TsconfigOutputResolution::Resolved(root.join("source/index.ts")),
                ),
            ] {
                let exports = export_key.map_or_else(
                    || serde_json::json!(target),
                    |key| serde_json::json!({key: target}),
                );
                write(
                    &package_root.join("package.json"),
                    &serde_json::json!({"exports": exports}).to_string(),
                );
                assert_eq!(
                    map(root, "./distribution/index.js", &["ts"]),
                    expected,
                    "package exports must resolve exactly: {extends} -> {target}"
                );
            }
        }
    }

    #[test]
    fn package_subpaths_resolve_through_simple_exports_without_bypassing_restrictions() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(
            &root.join("node_modules/shared-config/package.json"),
            r#"{"exports":{".":"./root.json","./build":"./build.json"}}"#,
        );
        write(
            &root.join("node_modules/shared-config/root.json"),
            r#"{"compilerOptions":{"outDir":"../../wrong-output"}}"#,
        );
        write(
            &root.join("node_modules/shared-config/build.json"),
            r#"{"compilerOptions":{"outDir":"../../distribution"}}"#,
        );
        write(
            &root.join("node_modules/shared-config/private.json"),
            r#"{"compilerOptions":{"outDir":"../../private-output"}}"#,
        );
        write(
            &root.join("node_modules/restricted-config/package.json"),
            r#"{"exports":"./root.json"}"#,
        );
        write(
            &root.join("node_modules/restricted-config/root.json"),
            r#"{"compilerOptions":{"outDir":"../../wrong-output"}}"#,
        );
        write(
            &root.join("node_modules/restricted-config/private.json"),
            r#"{"compilerOptions":{"outDir":"../../private-output"}}"#,
        );
        write(
            &root.join("tsconfig.build.json"),
            r#"{
                "extends":"shared-config/build",
                "compilerOptions":{"rootDir":"./source"}
            }"#,
        );
        write(
            &root.join("tsconfig.restricted.json"),
            r#"{
                "extends":"restricted-config/private",
                "compilerOptions":{"rootDir":"./source"}
            }"#,
        );
        write(
            &root.join("tsconfig.private.json"),
            r#"{"extends":"shared-config/private","compilerOptions":{"rootDir":"./source"}}"#,
        );
        write(&root.join("source/index.ts"), "export const value = 1;\n");

        let map = TsconfigOutputMap::from_project(root);
        assert_eq!(
            map.resolve_source_for_entry("./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("source/index.ts")),
            "the exact exported subpath should map through the configured build output"
        );
        assert_eq!(
            map.resolve_source_for_entry("./wrong-output/index.js", &["ts"]),
            TsconfigOutputResolution::Unconfigured,
            "a subpath must not use the package root config"
        );
        assert_eq!(
            map.resolve_source_for_entry("./private-output/index.js", &["ts"]),
            TsconfigOutputResolution::Unconfigured,
            "an unexported package subpath must not be loaded from disk"
        );
    }

    #[test]
    fn missing_local_extends_does_not_use_child_options() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(
            &root.join("tsconfig.build.json"),
            r#"{
                "extends":"./missing-config.json",
                "compilerOptions": {
                    "rootDir":"./source",
                    "outDir":"./distribution"
                }
            }"#,
        );
        write(&root.join("source/index.ts"), "export const value = 1;\n");

        assert_eq!(
            map(root, "./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Unconfigured
        );
        write(&root.join("missing-config.json"), "{}");
        assert_eq!(
            map(root, "./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("source/index.ts"))
        );
    }

    #[test]
    fn invalid_configs_fail_closed() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(&root.join("source/index.ts"), "export const value = 1;\n");
        write(
            &root.join("tsconfig.build.json"),
            "{ this is not valid JSONC",
        );

        assert_eq!(
            map(root, "./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Unconfigured
        );
        write(
            &root.join("tsconfig.build.json"),
            r#"{"compilerOptions":{"rootDir":"./source","outDir":"./distribution"}}"#,
        );
        assert_eq!(
            map(root, "./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("source/index.ts"))
        );

        let missing = tempfile::tempdir().expect("empty temporary project");
        assert_eq!(
            map(missing.path(), "./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Unconfigured
        );
    }
}
