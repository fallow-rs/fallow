//! `require.resolve` references follow only the `require` of the module.
//!
//! A local binding named `require` (a parameter or a nested declaration) is
//! not the CommonJS `require`, so a `.resolve` call on it references nothing.
//! A module-level `const require = createRequire(import.meta.url)` gives the
//! real function, so its calls still count.
//!
//! `import.meta.resolve` credits a package with the same specifier limits.
//! Code cannot rebind `import.meta`, so no shadow check applies.

use crate::tests::parse_ts as parse_source;

fn relative_references(source: &str) -> Vec<String> {
    parse_source(source)
        .dynamic_imports
        .iter()
        .map(|import| import.source.clone())
        .collect()
}

fn package_references(source: &str) -> Vec<String> {
    parse_source(source).package_path_references.to_vec()
}

#[test]
fn global_require_resolve_references_the_file_and_the_package() {
    let source = "require.resolve('./shim.js');\nrequire.resolve('some-pkg/package.json');\n";
    assert_eq!(relative_references(source), ["./shim.js"]);
    assert_eq!(package_references(source), ["some-pkg"]);
}

#[test]
fn module_level_create_require_still_references_the_file() {
    let source = "import { createRequire } from 'node:module';\n\
         const require = createRequire(import.meta.url);\n\
         export const shim = require.resolve('./shim.js');\n\
         export const pkg = require.resolve('some-pkg');\n";
    assert_eq!(relative_references(source), ["./shim.js"]);
    assert_eq!(package_references(source), ["some-pkg"]);
}

#[test]
fn a_require_parameter_shadows_the_module_require() {
    let source = "export function f(require) {\n\
           require.resolve('./shim.js');\n\
           return require.resolve('some-pkg');\n\
         }\n\
         export const g = (require) => require.resolve('./arrow.js');\n\
         export const h = function ({ require }) { return require.resolve('other-pkg'); };\n";
    assert!(
        relative_references(source).is_empty(),
        "a parameter named require is not the CommonJS require"
    );
    assert!(
        package_references(source).is_empty(),
        "a parameter named require is not the CommonJS require"
    );
}

#[test]
fn a_nested_declaration_shadows_the_module_require() {
    let source = "export function f() {\n\
           const require = makeLoader();\n\
           return [require.resolve('./shim.js'), require.resolve('some-pkg')];\n\
         }\n\
         if (globalThis.ready) {\n\
           let require = makeLoader();\n\
           require.resolve('./block.js');\n\
         }\n\
         require.resolve('./outside.js');\n";
    assert_eq!(
        relative_references(source),
        ["./outside.js"],
        "only the call outside the shadowing scopes references a file"
    );
    assert!(package_references(source).is_empty());
}

#[test]
fn a_package_helper_with_a_require_parameter_is_not_a_resolution_helper() {
    let source = "function resolveWith(require, name) {\n\
           return require.resolve(name);\n\
         }\n\
         resolveWith(localRequire, 'some-pkg');\n\
         function resolveNested(name) {\n\
           return [loader].map((require) => require.resolve(name));\n\
         }\n\
         resolveNested('nested-pkg');\n\
         function resolvePkg(name) {\n\
           return require.resolve(name);\n\
         }\n\
         resolvePkg('other-pkg');\n";
    assert_eq!(
        package_references(source),
        ["other-pkg"],
        "a helper that calls resolve on its own require parameter resolves nothing"
    );
}

#[test]
fn import_meta_resolve_references_the_package() {
    let source = "export const a = import.meta.resolve('open-pkg');\n\
         export const b = import.meta.resolve('@scope/data/package.json');\n\
         export const c = import.meta.resolve(`tpl-pkg`);\n";
    assert_eq!(
        package_references(source),
        ["open-pkg", "@scope/data", "tpl-pkg"]
    );
}

#[test]
fn import_meta_resolve_follows_the_require_resolve_subpath_limits() {
    let source = "export const a = import.meta.resolve('deep-pkg/dist/x.js');\n\
         export const b = import.meta.resolve('node:fs');\n";
    assert!(
        package_references(source).is_empty(),
        "a deep subpath or a protocol specifier credits no package"
    );
}

#[test]
fn a_local_meta_binding_is_not_import_meta() {
    let source = "const meta = { resolve: (s) => s };\n\
         export const a = meta.resolve('local-pkg');\n";
    assert!(package_references(source).is_empty());
}
