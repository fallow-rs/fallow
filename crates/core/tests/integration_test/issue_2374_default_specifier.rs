//! Issue #2374: `default` is one importable name that both sides may spell two
//! ways. An export declares it as `export default x` or as
//! `export { x as default }`; an import names it as `import x from './impl'`
//! or as `import { default as x } from './impl'`, and an ambient
//! `declare module '<specifier>' { export { default } from './impl' }` records
//! one named type-space import per specifier. Every pairing must credit the
//! target's default export.
//!
//! Two shapes must stay uncredited. A plain `export * from './impl'`: an ES
//! star re-export never forwards `default`, so the target's default export
//! keeps reporting. And a CSS Module class spelled `.default`: a stylesheet
//! exports class names rather than a module default, so only the member
//! accesses the consumer writes credit its classes.

use super::common::{create_config, fixture_path};

const FIXTURE: &str = "issue-2374-default-specifier";

fn unused_export_pairs(results: &fallow_core::results::AnalysisResults) -> Vec<(String, String)> {
    results
        .unused_exports
        .iter()
        .map(|e| {
            (
                e.export.path.to_string_lossy().replace('\\', "/"),
                e.export.export_name.clone(),
            )
        })
        .collect()
}

fn unused_defaults(results: &fallow_core::results::AnalysisResults) -> Vec<String> {
    unused_export_pairs(results)
        .into_iter()
        .filter(|(_, name)| name == "default")
        .map(|(path, _)| path)
        .collect()
}

#[test]
fn default_specifiers_credit_the_target_default_export() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let unused_defaults = unused_defaults(&results);
    let pairs = unused_export_pairs(&results);

    // `import { default as Aliased } from './alias-impl'` binds the same
    // export as `import Aliased from './alias-impl'`.
    // `import LocalDefault from './local-default'` binds the same export as
    // the `export { inner as default }` that declares it.
    // The `export { default } from` chain carries the binding through two hops
    // to `chain-deep.ts`, and the top of that chain is itself consumed with a
    // `default` specifier, so every hop keeps its credit.
    for path in [
        "src/alias-impl.ts",
        "src/local-default.ts",
        "src/chain-top.ts",
        "src/chain-mid.ts",
        "src/chain-deep.ts",
    ] {
        assert!(
            !unused_defaults.iter().any(|p| p.ends_with(path)),
            "{path}: a `default` specifier names the default export and must credit it; unused \
             defaults: {unused_defaults:?}"
        );
    }

    // A `default` specifier in a mixed statement leaves its named siblings
    // alone: `named` is used, `aliasSibling` is not.
    assert!(
        !pairs
            .iter()
            .any(|(path, name)| path.ends_with("src/alias-impl.ts") && name == "named"),
        "a named sibling of a `default` specifier keeps its credit: {pairs:?}"
    );
    assert!(
        pairs
            .iter()
            .any(|(path, name)| path.ends_with("src/alias-impl.ts") && name == "aliasSibling"),
        "an export no import names must keep reporting: {pairs:?}"
    );
}

#[test]
fn ambient_default_re_exports_credit_the_target_default_export() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let unused_defaults = unused_defaults(&results);
    let pairs = unused_export_pairs(&results);

    // `declare module 'untyped-default' { export { default } from './ambient-impl' }`
    // and the mixed `export { default as Impl, Y as Z } from './ambient-mixed'`
    // state that the target's default export is reachable through the declared
    // module id.
    for path in ["src/ambient-impl.ts", "src/ambient-mixed.ts"] {
        assert!(
            !unused_defaults.iter().any(|p| p.ends_with(path)),
            "{path}: an ambient `default` re-export must credit the target's default export; \
             unused defaults: {unused_defaults:?}"
        );
    }

    // The named half of the mixed statement was already credited and stays so,
    // while the sibling no specifier names keeps reporting. Both pin that the
    // fix credits the default without widening the ambient surface.
    assert!(
        !pairs
            .iter()
            .any(|(path, name)| path.ends_with("src/ambient-mixed.ts") && name == "Y"),
        "the named half of an ambient re-export keeps its credit: {pairs:?}"
    );
    for (file, name) in [
        ("src/ambient-impl.ts", "ambientSibling"),
        ("src/ambient-mixed.ts", "ambientMixedSibling"),
    ] {
        assert!(
            pairs
                .iter()
                .any(|(path, export)| path.ends_with(file) && export == name),
            "{file}: `{name}` is named by no ambient specifier and must keep reporting: {pairs:?}"
        );
    }
}

/// Deliberate negative control: a plain `export *` forwards every named export
/// and never `default`, so crediting a `default` specifier must not leak
/// through one.
#[test]
fn plain_star_re_export_still_does_not_forward_default() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let unused_defaults = unused_defaults(&results);
    let pairs = unused_export_pairs(&results);

    assert!(
        unused_defaults
            .iter()
            .any(|p| p.ends_with("src/star-impl.ts")),
        "a plain `export *` must not forward the target's default export; unused defaults: \
         {unused_defaults:?}"
    );
    // The named export the star does forward stays credited, so the control
    // proves the star edge is live rather than absent.
    assert!(
        !pairs
            .iter()
            .any(|(path, name)| path.ends_with("src/star-impl.ts") && name == "starNamed"),
        "the star re-export forwards named exports: {pairs:?}"
    );
}

/// Deliberate negative control: a CSS Module exports class names, so a class
/// spelled `.default` is an ordinary class. A plain
/// `import classes from './classes.module.css'` binds the whole class map and
/// names no single class, so only the members the consumer writes are
/// credited and `.default` must keep reporting.
#[test]
fn a_css_module_class_named_default_is_not_a_default_export() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let pairs = unused_export_pairs(&results);

    for name in ["default", "unusedClass"] {
        assert!(
            pairs
                .iter()
                .any(|(path, export)| path.ends_with("src/classes.module.css") && export == name),
            "`{name}` is accessed on no consumer and must keep reporting: {pairs:?}"
        );
    }
    // The class the consumer does write stays credited, so the control proves
    // the stylesheet edge is live rather than absent.
    assert!(
        !pairs
            .iter()
            .any(|(path, name)| path.ends_with("src/classes.module.css") && name == "usedClass"),
        "a class the consumer accesses keeps its credit: {pairs:?}"
    );
}

/// A CommonJS `exports.default = x` declares the same binding
/// `import x from './m'` reads, and the extractor records it as
/// `Named("default")` exactly like `export { x as default }`. Crediting it is
/// deliberate: the graph cannot tell the two apart, and leaving it uncredited
/// reported the default of every transpiled CommonJS module as unused.
#[test]
fn a_commonjs_default_property_export_answers_a_plain_default_import() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let unused_defaults = unused_defaults(&results);
    let pairs = unused_export_pairs(&results);

    assert!(
        !unused_defaults
            .iter()
            .any(|p| p.ends_with("src/cjs-default.cjs")),
        "`exports.default` is the binding a plain default import reads; unused defaults: \
         {unused_defaults:?}"
    );
    assert!(
        pairs
            .iter()
            .any(|(path, name)| path.ends_with("src/cjs-default.cjs") && name == "cjsSibling"),
        "a sibling property export no import names must keep reporting: {pairs:?}"
    );
}

/// A named re-export chain forwards exactly the specifiers written on it, so
/// crediting the default through two hops must not launder the sibling.
#[test]
fn default_re_export_chain_does_not_launder_named_siblings() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let pairs = unused_export_pairs(&results);

    assert!(
        pairs
            .iter()
            .any(|(path, name)| path.ends_with("src/chain-deep.ts") && name == "chainSibling"),
        "`chainSibling` is on no `export {{ default }}` specifier and must keep reporting: \
         {pairs:?}"
    );
}
