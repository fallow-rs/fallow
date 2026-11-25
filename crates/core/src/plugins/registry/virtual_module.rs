//! The rules that credit the package behind the virtual and generated
//! modules of a plugin, such as `@sveltejs/kit` for `$app/state` and
//! `./$types`.

use std::path::Path;

use crate::extract::ModuleInfo;

/// The extensions that an import of a generated module can carry
/// (`./$types.js`).
const GENERATED_IMPORT_EXTENSIONS: &[&str] = &[".js", ".ts"];

/// How a [`VirtualModuleProviderRule`] matches an import specifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VirtualModuleMatch {
    /// A `Plugin::virtual_module_prefixes` entry. A prefix with a trailing
    /// `/`, such as `$app/`, matches each specifier below it. A prefix
    /// without one, such as `$service-worker`, matches that module and its
    /// subpaths, not `$service-workerx`.
    Prefix(String),
    /// A `Plugin::generated_import_patterns` entry such as `/$types`. It
    /// matches a relative specifier whose directory part has only `.` and
    /// `..` segments, after one `.js` or `.ts` extension is stripped:
    /// `./$types` and `../$types.js`, not `./lib/$types`, which names a
    /// project file.
    Generated(String),
}

/// One import-specifier rule crediting the package that provides a plugin's
/// virtual or generated modules. See [`Plugin::virtual_module_package`].
///
/// [`Plugin::virtual_module_package`]: crate::plugins::Plugin::virtual_module_package
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualModuleProviderRule {
    /// The plugin that declares the package.
    pub plugin: String,
    /// The project-relative root of the package the plugin is active in.
    /// Empty for the project root.
    pub scope: String,
    /// The specifier matcher a virtual-module or generated-import entry of
    /// the plugin contributed.
    pub matcher: VirtualModuleMatch,
    /// The package a matched import credits.
    pub package: String,
}

impl VirtualModuleProviderRule {
    /// Whether this rule covers `specifier` written in the file at `path`.
    ///
    /// A rule applies only to the files under the package root where its
    /// plugin is active, so a virtual-module import in another workspace
    /// credits no package. A match hides an unused package, so the match is
    /// narrower than the unresolved-import filter.
    #[must_use]
    pub fn matches(&self, root: &Path, path: &Path, specifier: &str) -> bool {
        if !self.scope.is_empty() && !path.starts_with(root.join(&self.scope)) {
            return false;
        }
        match &self.matcher {
            VirtualModuleMatch::Prefix(prefix) => names_virtual_module(prefix, specifier),
            VirtualModuleMatch::Generated(pattern) => names_generated_module(pattern, specifier),
        }
    }
}

fn names_generated_module(pattern: &str, specifier: &str) -> bool {
    let bare = GENERATED_IMPORT_EXTENSIONS
        .iter()
        .find_map(|extension| specifier.strip_suffix(extension))
        .unwrap_or(specifier);
    let Some((dir, name)) = bare.rsplit_once('/') else {
        return false;
    };
    name == pattern.trim_start_matches('/')
        && dir
            .split('/')
            .all(|segment| segment == "." || segment == "..")
}

fn names_virtual_module(prefix: &str, specifier: &str) -> bool {
    if prefix.ends_with('/') {
        return specifier.starts_with(prefix);
    }
    specifier
        .strip_prefix(prefix)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// Every literal specifier of the module: static imports, re-exports,
/// literal dynamic imports and `require()` calls. Resolved or not, each is
/// evidence that the file uses the framework module it names.
pub fn module_specifiers(module: &ModuleInfo) -> impl Iterator<Item = &str> {
    module
        .imports
        .iter()
        .map(|import| import.source.as_str())
        .chain(
            module
                .re_exports
                .iter()
                .map(|re_export| re_export.source.as_str()),
        )
        .chain(
            module
                .dynamic_imports
                .iter()
                .map(|import| import.source.as_str()),
        )
        .chain(module.require_calls.iter().map(|call| call.source.as_str()))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{VirtualModuleMatch, VirtualModuleProviderRule};

    fn rule(matcher: VirtualModuleMatch) -> VirtualModuleProviderRule {
        VirtualModuleProviderRule {
            plugin: "sveltekit".to_string(),
            scope: String::new(),
            matcher,
            package: "@sveltejs/kit".to_string(),
        }
    }

    fn matches(matcher: VirtualModuleMatch, specifier: &str) -> bool {
        let root = Path::new("/project");
        rule(matcher).matches(root, &root.join("src/routes/+page.ts"), specifier)
    }

    #[test]
    fn a_prefix_without_a_slash_names_one_module_and_its_subpaths() {
        let prefix = || VirtualModuleMatch::Prefix("$service-worker".to_string());
        assert!(matches(prefix(), "$service-worker"));
        assert!(matches(prefix(), "$service-worker/build"));
        assert!(!matches(prefix(), "$service-workerx"));
    }

    #[test]
    fn a_prefix_with_a_slash_names_each_module_below_it() {
        let prefix = || VirtualModuleMatch::Prefix("$app/".to_string());
        assert!(matches(prefix(), "$app/state"));
        assert!(!matches(prefix(), "$app"));
        assert!(!matches(prefix(), "$application/state"));
    }

    #[test]
    fn a_generated_import_must_be_relative() {
        let generated = || VirtualModuleMatch::Generated("/$types".to_string());
        assert!(matches(generated(), "./$types"));
        assert!(matches(generated(), "../$types.js"));
        assert!(matches(generated(), "../../$types.ts"));
        assert!(!matches(generated(), "some-pkg/$types"));
        assert!(!matches(generated(), "./$typesx"));
        assert!(!matches(generated(), "./lib/$types"));
        assert!(!matches(generated(), "../shared/$types.js"));
    }

    #[test]
    fn a_scoped_rule_covers_only_its_package() {
        let root = Path::new("/project");
        let scoped = VirtualModuleProviderRule {
            scope: "packages/app".to_string(),
            ..rule(VirtualModuleMatch::Prefix("$app/".to_string()))
        };
        assert!(scoped.matches(root, &root.join("packages/app/src/a.ts"), "$app/state"));
        assert!(!scoped.matches(root, &root.join("packages/other/src/a.ts"), "$app/state"));
    }
}
