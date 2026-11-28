//! Resolve the alias value of a bundler config to `(find, replacement,
//! is_bare)` pairs, through local constants, relative imports and spreads.

use std::path::{Path, PathBuf};

use oxc_ast::ast::*;
use rustc_hash::{FxHashMap, FxHashSet};

use super::{
    MAX_ALIAS_RESOLVE_DEPTH, PathScope, alias_entry, alias_replacement_kinded, extract_from_source,
    find_exported_init, find_relative_import_binding, find_variable_init_expression,
    property_key_to_string, resolve_sibling_module,
};

/// Maximum expressions the alias resolver reads for one alias value. A config
/// can spread one name many times at each level, so the depth bound alone
/// allows an exponential number of reads.
const MAX_ALIAS_RESOLVE_STEPS: usize = 4_096;

/// Maximum pairs that a list takes from its spreads. Spreads of spreads
/// multiply the pairs, so a short config can otherwise collect millions of
/// copies. Pairs written out in the list itself do not count against this
/// cap, because the source bounds them. Real configs declare fewer than a
/// hundred aliases, and generated ones a few hundred.
const MAX_ALIAS_PAIRS: usize = 4_096;

/// One resolved alias: `(find, replacement, is_bare)`.
type AliasPair = (String, String, bool);

/// The shared state of one alias resolution.
#[derive(Default)]
pub(super) struct AliasResolveState {
    /// Sibling files already read, to break import cycles.
    visited: FxHashSet<PathBuf>,
    /// The pairs of each identifier, by file, name and depth. A config can
    /// spread one name many times at each level, so each name is resolved
    /// once per depth and the later spreads reuse the result.
    resolved: FxHashMap<(PathBuf, String, usize), Vec<AliasPair>>,
    /// Expressions read so far, bounded by [`MAX_ALIAS_RESOLVE_STEPS`].
    steps: usize,
}

impl AliasResolveState {
    /// Count one read expression. Returns `false` when the step budget is
    /// used up.
    fn step(&mut self) -> bool {
        if self.steps >= MAX_ALIAS_RESOLVE_STEPS {
            return false;
        }
        self.steps += 1;
        true
    }
}

/// Add the pairs of a spread to `pairs`, up to [`MAX_ALIAS_PAIRS`] pairs in
/// total.
fn extend_alias_pairs(pairs: &mut Vec<(String, String, bool)>, more: Vec<(String, String, bool)>) {
    let room = MAX_ALIAS_PAIRS.saturating_sub(pairs.len());
    pairs.extend(more.into_iter().take(room));
}

/// Resolve an alias expression into `(find, replacement, is_bare)` tuples,
/// following identifiers and expanding spreads.
///
/// Beyond the inline object (`{ '@': './src' }`) and array
/// (`[{ find, replacement }]`) forms, this handles the indirection shapes from
/// issue #811:
/// - an identifier bound to a local `const NAME = [...] | {...}`,
/// - an identifier imported from a relative sibling file
///   (`import { sharedAliases } from "./vite.shared.js"`), read one hop and
///   parsed for `export const NAME` / `export default` / `export { NAME }`,
/// - array spread elements (`[...a, ...b]`) and object spread properties
///   (`{ ...a, '@': './src' }`), each resolved recursively.
///
/// `config_path` is the file `expr` lives in (used to resolve relative sibling
/// imports). `state` breaks import cycles and bounds the work, see
/// [`AliasResolveState`]. `depth` bounds identifier indirection via
/// [`MAX_ALIAS_RESOLVE_DEPTH`].
pub(super) fn resolve_alias_pairs_kinded(
    program: &Program,
    config_path: &Path,
    expr: &Expression,
    state: &mut AliasResolveState,
    depth: usize,
) -> Vec<(String, String, bool)> {
    if !state.step() {
        return Vec::new();
    }
    match expr {
        Expression::ParenthesizedExpression(paren) => {
            resolve_alias_pairs_kinded(program, config_path, &paren.expression, state, depth)
        }
        Expression::TSAsExpression(ts_as) => {
            resolve_alias_pairs_kinded(program, config_path, &ts_as.expression, state, depth)
        }
        Expression::TSSatisfiesExpression(ts_sat) => {
            resolve_alias_pairs_kinded(program, config_path, &ts_sat.expression, state, depth)
        }
        Expression::ObjectExpression(obj) => {
            resolve_object_alias_pairs_kinded(program, config_path, obj, state, depth)
        }
        Expression::ArrayExpression(arr) => {
            resolve_array_alias_pairs_kinded(program, config_path, arr, state, depth)
        }
        Expression::Identifier(id) => {
            resolve_identifier_alias_pairs(program, config_path, id.name.as_str(), state, depth)
        }
        _ => Vec::new(),
    }
}

/// Resolve object-form alias pairs (`{ '@': './src', ...spread }`), expanding
/// spread properties recursively.
fn resolve_object_alias_pairs_kinded(
    program: &Program,
    config_path: &Path,
    obj: &ObjectExpression,
    state: &mut AliasResolveState,
    depth: usize,
) -> Vec<(String, String, bool)> {
    let mut pairs = Vec::new();
    for prop in &obj.properties {
        match prop {
            ObjectPropertyKind::ObjectProperty(prop) => {
                if let Some(find) = property_key_to_string(&prop.key)
                    && let Some((replacement, is_bare)) =
                        alias_replacement_kinded(&prop.value, PathScope::Program(program))
                {
                    pairs.push((find, replacement, is_bare));
                }
            }
            // `{ ...sharedAliases, '@': './src' }`
            ObjectPropertyKind::SpreadProperty(spread) => {
                let spread_pairs = resolve_alias_pairs_kinded(
                    program,
                    config_path,
                    &spread.argument,
                    state,
                    depth,
                );
                extend_alias_pairs(&mut pairs, spread_pairs);
            }
        }
    }
    pairs
}

/// Resolve array-form alias pairs (`[{ find, replacement }, ...spread]`),
/// expanding spread elements recursively.
fn resolve_array_alias_pairs_kinded(
    program: &Program,
    config_path: &Path,
    arr: &ArrayExpression,
    state: &mut AliasResolveState,
    depth: usize,
) -> Vec<(String, String, bool)> {
    let mut pairs = Vec::new();
    for element in &arr.elements {
        match element {
            // `[...sharedAliases, { find, replacement }]`
            ArrayExpressionElement::SpreadElement(spread) => {
                let spread_pairs = resolve_alias_pairs_kinded(
                    program,
                    config_path,
                    &spread.argument,
                    state,
                    depth,
                );
                extend_alias_pairs(&mut pairs, spread_pairs);
            }
            _ => {
                if let Some(Expression::ObjectExpression(obj)) = element.as_expression()
                    && let Some(pair) =
                        alias_entry::object_alias_pair(obj, PathScope::Program(program))
                {
                    pairs.push(pair);
                }
            }
        }
    }
    pairs
}

/// Resolve an identifier used as an alias value to its literal pairs, first by
/// local `const`/`let`/`var` binding, then by a one-hop relative import.
fn resolve_identifier_alias_pairs(
    program: &Program,
    config_path: &Path,
    name: &str,
    state: &mut AliasResolveState,
    depth: usize,
) -> Vec<(String, String, bool)> {
    if depth >= MAX_ALIAS_RESOLVE_DEPTH {
        return Vec::new();
    }
    let key = (config_path.to_path_buf(), name.to_string(), depth);
    if let Some(pairs) = state.resolved.get(&key) {
        return pairs.clone();
    }
    let pairs = resolve_identifier_alias_pairs_uncached(program, config_path, name, state, depth);
    state.resolved.insert(key, pairs.clone());
    pairs
}

fn resolve_identifier_alias_pairs_uncached(
    program: &Program,
    config_path: &Path,
    name: &str,
    state: &mut AliasResolveState,
    depth: usize,
) -> Vec<(String, String, bool)> {
    // Local `const NAME = [...] | {...}` (or `const NAME = otherIdentifier`).
    if let Some(init) = find_variable_init_expression(program, name) {
        return resolve_alias_pairs_kinded(program, config_path, init, state, depth + 1);
    }
    // `import { NAME } from "./sibling"` / `import NAME from "./sibling"`.
    let Some((specifier, imported_name)) = find_relative_import_binding(program, name) else {
        return Vec::new();
    };
    resolve_imported_alias_pairs(
        config_path,
        &specifier,
        imported_name.as_deref(),
        state,
        depth + 1,
    )
}

/// Read a relative sibling file and resolve the alias literal it exports under
/// `imported_name` (`None` = default export).
fn resolve_imported_alias_pairs(
    config_path: &Path,
    specifier: &str,
    imported_name: Option<&str>,
    state: &mut AliasResolveState,
    depth: usize,
) -> Vec<(String, String, bool)> {
    let Some((sibling_path, sibling_source)) = resolve_sibling_module(config_path, specifier)
    else {
        return Vec::new();
    };
    if !state.visited.insert(sibling_path.clone()) {
        return Vec::new();
    }
    extract_from_source(&sibling_source, &sibling_path, |program| {
        let init = find_exported_init(program, imported_name)?;
        let pairs = resolve_alias_pairs_kinded(program, &sibling_path, init, state, depth);
        (!pairs.is_empty()).then_some(pairs)
    })
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;
    use std::path::Path;

    use super::super::{extract_config_aliases, extract_config_aliases_kinded};
    use super::MAX_ALIAS_PAIRS;

    #[test]
    fn aliases_repeated_self_spreads_resolve_in_bounded_time() {
        // Without a memo, each level resolves every spread again, so 14
        // spreads cost 14^8 lookups over the eight levels.
        let spreads = vec!["...shared"; 14].join(", ");
        let source = format!(
            "const shared = [{spreads}];\n\
             export default {{ resolve: {{ alias: [{{ find: '@', replacement: './src' }}, ...shared] }} }};"
        );
        let started = std::time::Instant::now();
        let got =
            extract_config_aliases(&source, Path::new("vite.config.js"), &["resolve", "alias"]);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "took {:?}",
            started.elapsed()
        );
        assert_eq!(got, vec![("@".to_string(), "./src".to_string())]);
    }

    #[test]
    fn aliases_spread_fan_out_stops_at_the_pair_budget() {
        // Six levels of ten spreads each give 10^6 copies of one pair.
        let mut source = String::from("const s0 = [{ find: '@', replacement: './src' }];\n");
        for level in 1..=6 {
            let spreads = vec![format!("...s{}", level - 1); 10].join(", ");
            writeln!(source, "const s{level} = [{spreads}];")
                .expect("writing to a String cannot fail");
        }
        source.push_str("export default { resolve: { alias: s6 } };\n");
        let got = extract_config_aliases_kinded(
            &source,
            Path::new("vite.config.js"),
            &["resolve", "alias"],
        );
        assert!(!got.is_empty());
        assert!(got.len() <= MAX_ALIAS_PAIRS, "{} pairs", got.len());
    }

    #[test]
    fn aliases_written_out_in_full_are_all_kept() {
        let entries = 600;
        let object = (0..entries)
            .map(|index| format!("'@a{index}': './src/a{index}'"))
            .collect::<Vec<_>>()
            .join(", ");
        let source = format!("export default {{ resolve: {{ alias: {{ {object} }} }} }};");
        let got = extract_config_aliases_kinded(
            &source,
            Path::new("vite.config.js"),
            &["resolve", "alias"],
        );
        assert_eq!(got.len(), entries);
        let array = (0..entries)
            .map(|index| format!("{{ find: '@a{index}', replacement: './src/a{index}' }}"))
            .collect::<Vec<_>>()
            .join(", ");
        let source = format!(
            "const shared = [{array}];\nexport default {{ resolve: {{ alias: [...shared] }} }};"
        );
        let got = extract_config_aliases_kinded(
            &source,
            Path::new("vite.config.js"),
            &["resolve", "alias"],
        );
        assert_eq!(got.len(), entries);
    }
}
