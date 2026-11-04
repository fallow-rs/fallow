//! Unlisted dependencies that a config needs, without a source import.

use std::path::Path;

use fallow_config::{PackageJson, ResolvedConfig};
use rustc_hash::FxHashSet;

use crate::plugins::AggregatedPluginResult;
use crate::results::{ImportSite, UnlistedDependency};

use super::{UnlistedDependencyContext, should_skip_unlisted_package, types_package_name};

/// Add each package that a config needs and that no manifest the config can
/// use lists.
///
/// Node resolves a package that a tool loads from the config directory
/// upward, so the manifest that owns the config and every ancestor manifest
/// count. A sibling manifest does not.
pub(super) fn extend(
    mut unlisted: Vec<UnlistedDependency>,
    plugin_result: Option<&AggregatedPluginResult>,
    ctx: &UnlistedDependencyContext<'_>,
) -> Vec<UnlistedDependency> {
    let required = plugin_result.map_or(&[][..], |pr| pr.required_dependencies.as_slice());
    for requirement in required {
        let package_name = requirement.package_name.as_str();
        if (requirement.dev_only && ctx.config.production)
            || config_path_is_ignored(&requirement.config_path, ctx.config)
            || should_skip_unlisted_package(package_name, ctx)
            || ctx.ignore_deps.suppresses(package_name)
            || config_dependency_is_declared(package_name, &requirement.config_path, ctx)
        {
            continue;
        }
        let site = ImportSite {
            path: requirement.config_path.clone(),
            line: requirement.line,
            col: requirement.col,
        };
        match unlisted
            .iter_mut()
            .find(|finding| finding.package_name == package_name)
        {
            Some(finding) => {
                if !finding
                    .imported_from
                    .iter()
                    .any(|known| known.path == site.path)
                {
                    finding.imported_from.push(site);
                    finding
                        .imported_from
                        .sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));
                }
            }
            None => unlisted.push(UnlistedDependency {
                package_name: package_name.to_owned(),
                imported_from: vec![site],
            }),
        }
    }
    unlisted
}

/// Whether `ignorePatterns` covers the config file.
fn config_path_is_ignored(config_path: &Path, config: &ResolvedConfig) -> bool {
    config_path
        .strip_prefix(&config.root)
        .is_ok_and(|relative| config.ignore_patterns.is_match(relative))
}

/// Whether the manifest that owns `config_path`, an ancestor manifest, or the
/// root manifest lists `package_name` or its `@types` package.
///
/// The walk reads each `package.json` from the config directory up to the
/// project root. A manifest that owns no analyzed file is not in the
/// workspace map, but a tool that runs from its directory still resolves
/// the packages that it lists.
fn config_dependency_is_declared(
    package_name: &str,
    config_path: &Path,
    ctx: &UnlistedDependencyContext<'_>,
) -> bool {
    let types_name = types_package_name(package_name);
    let declares =
        |deps: &FxHashSet<String>| deps.contains(package_name) || deps.contains(&types_name);
    if declares(ctx.all_deps)
        || ctx
            .ws_dep_map
            .iter()
            .filter(|workspace| config_path.starts_with(&workspace.root))
            .any(|workspace| declares(&workspace.deps))
    {
        return true;
    }
    config_path
        .ancestors()
        .skip(1)
        .take_while(|dir| dir.starts_with(&ctx.config.root))
        .filter_map(|dir| PackageJson::load(&dir.join("package.json")).ok())
        .any(|pkg| {
            pkg.all_dependency_names()
                .iter()
                .any(|name| name == package_name || *name == types_name)
        })
}
