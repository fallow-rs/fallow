/// How a [`VirtualModuleProviderRule`] matches an import specifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VirtualModuleMatch {
    /// A `Plugin::virtual_module_prefixes` entry, matched with the same
    /// semantics as the unresolved-import suppression (`matches_virtual_prefix`).
    Prefix(String),
    /// A `Plugin::generated_import_patterns` entry, matched as a specifier
    /// suffix after one `.js` or `.ts` extension is stripped.
    Generated(String),
}

/// One import-specifier rule crediting the package that provides a plugin's
/// virtual or generated modules. See [`Plugin::virtual_module_package`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualModuleProviderRule {
    /// The project-relative root of the package the plugin is active in.
    /// Empty for the project root.
    pub scope: String,
    /// The specifier matcher a virtual-module or generated-import entry of
    /// the plugin contributed.
    pub matcher: VirtualModuleMatch,
    /// The package a matched import credits.
    pub package: String,
}
