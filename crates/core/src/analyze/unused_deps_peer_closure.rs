//! The peer-dependency closure of the unused-dependency check and of the
//! dependency trace, and the packages that start it.

use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_config::PackageJson;
use fallow_types::trace::PeerClosureSeeds;

use crate::graph::ModuleGraph;

use super::{
    WorkspaceOwnershipIndex, closure_listed_names, collect_workspace_used_packages,
    credited_plugin_tooling, is_package_json_ignored_under,
};

pub(super) struct PeerDependencyResolver {
    cache: FxHashMap<(PathBuf, String), Vec<InstalledPeer>>,
}

/// One entry of an installed package's `peerDependencies`.
#[derive(Clone)]
struct InstalledPeer {
    name: String,
    /// `peerDependenciesMeta.<name>.optional` is set.
    optional: bool,
}

impl PeerDependencyResolver {
    pub(super) fn new() -> Self {
        Self {
            cache: FxHashMap::default(),
        }
    }

    /// Every package that a seed package lists in `peerDependencies`, directly
    /// or through another credited peer, mapped to the packages that list it.
    ///
    /// Optional peers (`peerDependenciesMeta.<name>.optional`) count too: a
    /// project lists an optional peer of a package it uses to turn on a feature
    /// of that package, which loads the peer at runtime where the import graph
    /// does not see it. Only used packages seed the closure, so the peers of an
    /// unused package get no credit.
    ///
    /// A required peer always passes credit on to its own peers, because the
    /// host needs it. An optional peer passes credit on only when `listed`
    /// (the names that the root and workspace manifests declare) contains it.
    /// An optional peer that the project does not list is not turned on, so
    /// its peers get no credit through it.
    pub(super) fn peer_dependency_closure<'b>(
        &mut self,
        package_root: &Path,
        seeds: impl IntoIterator<Item = &'b str>,
        listed: &FxHashSet<String>,
    ) -> FxHashMap<String, Vec<String>> {
        let mut hosts_by_peer: FxHashMap<String, Vec<String>> = FxHashMap::default();
        let mut expanded = FxHashSet::default();
        let mut queue: Vec<String> = seeds.into_iter().map(str::to_string).collect();

        while let Some(package_name) = queue.pop() {
            if !expanded.insert(package_name.clone()) {
                continue;
            }

            for peer in self.peer_dependencies_for(package_root, &package_name) {
                if !peer.optional || listed.contains(peer.name.as_str()) {
                    queue.push(peer.name.clone());
                }
                hosts_by_peer
                    .entry(peer.name)
                    .or_default()
                    .push(package_name.clone());
            }
        }

        hosts_by_peer
    }

    fn peer_dependencies_for(
        &mut self,
        package_root: &Path,
        package_name: &str,
    ) -> Vec<InstalledPeer> {
        let key = (package_root.to_path_buf(), package_name.to_string());
        if let Some(cached) = self.cache.get(&key) {
            return cached.clone();
        }

        let peer_dependencies: Vec<InstalledPeer> =
            find_installed_package_json(package_root, package_name)
                .and_then(|path| PackageJson::load(&path).ok())
                .map(|pkg| {
                    pkg.peer_dependency_names()
                        .into_iter()
                        .map(|name| InstalledPeer {
                            optional: pkg.peer_dependency_is_optional(&name),
                            name,
                        })
                        .collect()
                })
                .unwrap_or_default();

        self.cache.insert(key, peer_dependencies.clone());
        peer_dependencies
    }
}

/// The packages that give `package_name` peer-dependency credit in the
/// unused-dependency check: each one is used, or credited itself, and lists
/// `package_name` in its installed `peerDependencies`, required or optional.
///
/// The check runs one closure for the root manifest and one for each
/// workspace, and this function runs the same closures. The root closure
/// starts from every package that the import graph records and looks up the
/// installed manifests from `project_root` and its ancestors. A workspace
/// closure starts from the packages that the files of that workspace import
/// and looks up the installed manifests from the workspace root. In every
/// closure, an optional peer passes credit on only when the root
/// `package.json` or a workspace `package.json` lists it, as in the root and
/// workspace checks. A workspace `package.json` that `ignore_patterns` matches
/// or that does not load does not count and gets no closure, as in those
/// checks. The result is the union of the hosts, sorted, and empty when no
/// closure credits `package_name`.
///
/// `extra_seeds` holds the packages that start the closures although no file
/// imports them, as `peer_closure_seeds` collects them for the check.
pub fn peer_dependency_hosts(
    graph: &ModuleGraph,
    project_root: &Path,
    workspace_roots: &[&Path],
    ignore_patterns: &fallow_config::IgnorePatternSet,
    package_name: &str,
    extra_seeds: &PeerClosureSeeds,
) -> Vec<String> {
    let root_declared = PackageJson::load(&project_root.join("package.json"))
        .map(|pkg| pkg.all_dependency_names())
        .unwrap_or_default();
    let workspace_manifests: Vec<(&Path, Vec<String>)> = workspace_roots
        .iter()
        .copied()
        .filter_map(|ws_root| {
            let pkg_path = ws_root.join("package.json");
            if is_package_json_ignored_under(&pkg_path, project_root, ignore_patterns) {
                return None;
            }
            let pkg = PackageJson::load(&pkg_path).ok()?;
            Some((ws_root, pkg.all_dependency_names()))
        })
        .collect();
    let listed = closure_listed_names(
        &root_declared,
        workspace_manifests
            .iter()
            .flat_map(|(_, declared)| declared.iter()),
    );
    let workspace_roots: Vec<&Path> = workspace_manifests
        .iter()
        .map(|(ws_root, _)| *ws_root)
        .collect();
    let mut resolver = PeerDependencyResolver::new();
    let mut hosts = resolver
        .peer_dependency_closure(
            project_root,
            graph
                .package_usage
                .keys()
                .map(String::as_str)
                .chain(extra_seeds.for_manifest(&project_root.join("package.json"))),
            &listed,
        )
        .remove(package_name)
        .unwrap_or_default();
    let ownership = WorkspaceOwnershipIndex::new(graph, &workspace_roots);
    let used_by_workspace = collect_workspace_used_packages(graph, &workspace_roots, &ownership);
    for ws_root in &workspace_roots {
        if let Some(ws_hosts) = resolver
            .peer_dependency_closure(
                ws_root,
                used_by_workspace
                    .get(ws_root)
                    .into_iter()
                    .flatten()
                    .copied()
                    .chain(extra_seeds.for_manifest(&ws_root.join("package.json"))),
                &listed,
            )
            .remove(package_name)
        {
            hosts.extend(ws_hosts);
        }
    }
    hosts.sort_unstable();
    hosts.dedup();
    hosts
}

fn find_installed_package_json(package_root: &Path, package_name: &str) -> Option<PathBuf> {
    for base in package_root.ancestors() {
        let candidate = node_modules_package_json(base, package_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn node_modules_package_json(base: &Path, package_name: &str) -> PathBuf {
    let mut path = base.join("node_modules");
    for segment in package_name.split('/') {
        path.push(segment);
    }
    path.join("package.json")
}

/// The packages that start every peer-dependency closure although no file
/// imports them.
///
/// A package that a script runs, a package that a plugin config names, and a
/// plugin tooling package with evidence of use are in use. Thus their peers
/// get credit, as the peers of an imported package do. Plugin tooling without
/// evidence and catalogue tooling do not start a closure, because the project
/// possibly does not use them.
pub(super) fn peer_closure_shared_seeds(
    plugin_result: &crate::plugins::AggregatedPluginResult,
) -> FxHashSet<&str> {
    plugin_result
        .script_used_packages
        .iter()
        .chain(&plugin_result.referenced_dependencies)
        .map(String::as_str)
        .chain(credited_plugin_tooling(plugin_result))
        .collect()
}

/// The seeds of the peer-dependency closures that the unused-dependency check
/// adds to the imported packages, for the dependency trace.
///
/// The shared seeds come from [`peer_closure_shared_seeds`]. A package that a
/// plugin reads from one `package.json` seeds the closure of that manifest
/// only.
pub fn peer_closure_seeds(
    plugin_result: &crate::plugins::AggregatedPluginResult,
) -> PeerClosureSeeds {
    let mut seeds = PeerClosureSeeds::from_shared(
        peer_closure_shared_seeds(plugin_result)
            .into_iter()
            .map(str::to_string),
    );
    for (manifest, name) in &plugin_result.package_referenced_dependencies {
        seeds.push_manifest_seed(manifest.clone(), name.clone());
    }
    seeds
}

/// The packages that start a peer-dependency closure in addition to the
/// imported packages: the shared seeds and the seeds of one manifest.
pub(super) struct PeerSeedInput<'s, 'a> {
    pub(super) shared: &'s FxHashSet<&'a str>,
    pub(super) manifest: Option<&'s FxHashSet<&'a str>>,
}

impl<'a> PeerSeedInput<'_, 'a> {
    pub(super) fn iter(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.shared
            .iter()
            .chain(self.manifest.into_iter().flatten())
            .copied()
    }
}
