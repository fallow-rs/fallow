/// JSX prop names that the extractor follows through a spread of a local
/// object literal (`const props = { dangerouslySetInnerHTML: x }; <div {...props} />`).
///
/// The security catalogue owns the `jsx-attr` sink rows. A catalogue test
/// asserts that every `jsx-attr` row pattern is in this list, so a new JSX sink
/// row cannot silently lose spread coverage.
pub const JSX_SPREAD_SINK_PROPS: &[&str] = &[
    "dangerouslySetInnerHTML",
    "srcDoc",
    "srcdoc",
    "injectedJavaScript",
];

/// Node.js built-in modules whose bindings resolve to a `node:<module>.<member>`
/// sink callee path. The `node:` root is not a valid identifier, so the path
/// proves the import provenance on its own.
///
/// The extractor writes these paths for sink sites, and the policy effect
/// lookup builds the same paths for callee uses, so both read this list.
pub const NODE_BINDING_MODULES: &[&str] = &["vm", "child_process", "fs"];

/// The `node:` root of an import or `require()` source when the module is in
/// [`NODE_BINDING_MODULES`]. `vm` and `node:vm` both give `node:vm`.
/// `fs/promises` gives `node:fs`, because its members take the same paths.
#[must_use]
pub fn node_binding_module(source: &str) -> Option<String> {
    let module = source.strip_prefix("node:").unwrap_or(source);
    let module = if module == "fs/promises" {
        "fs"
    } else {
        module
    };
    NODE_BINDING_MODULES
        .contains(&module)
        .then(|| format!("node:{module}"))
}

#[cfg(test)]
mod tests {
    use super::node_binding_module;

    #[test]
    fn node_binding_module_admits_the_prefix_and_fs_promises() {
        assert_eq!(node_binding_module("vm").as_deref(), Some("node:vm"));
        assert_eq!(
            node_binding_module("node:child_process").as_deref(),
            Some("node:child_process")
        );
        assert_eq!(
            node_binding_module("fs/promises").as_deref(),
            Some("node:fs")
        );
        assert_eq!(node_binding_module("fs-extra"), None);
        assert_eq!(node_binding_module("path"), None);
    }
}
