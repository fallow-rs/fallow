//! Resolve the options value of a Module Federation plugin call or config to
//! the object literals it is built from.

use std::path::{Path, PathBuf};

use oxc_ast::ast::{
    Argument, CallExpression, Expression, ObjectExpression, ObjectPropertyKind, Program,
};
use rustc_hash::FxHashMap;

use super::{
    FederationConfig, FederationKey, IDENTITY_WRAPPERS, UnreadDeclaration, config_parser,
    push_unique, read_exposes, read_remotes, read_shared, unwrap_expression,
};

/// The options each name resolved to, by file, name and depth, with whether
/// the name resolved.
pub(super) type OptionsMemo = FxHashMap<(PathBuf, String, usize), (bool, ResolvedOptions)>;

/// How many steps of indirection the options resolver follows. A binding, a
/// spread, an `Object.assign` argument and an import are one step each.
const MAX_OPTIONS_DEPTH: usize = 4;

/// What one Federation options value declares, read from every object literal
/// the value resolves to.
#[derive(Debug, Default, Clone)]
pub(super) struct ResolvedOptions {
    pub(super) config: FederationConfig,
    pub(super) unread: Vec<UnreadDeclaration>,
    /// The Federation keys the readable part declares.
    pub(super) declared: Vec<FederationKey>,
    /// Whether a spread or an `Object.assign` argument did not resolve, so the
    /// value can declare more than what was read.
    pub(super) unreadable_spread: bool,
    /// Whether a followed relative import or `require` target could not be
    /// read.
    pub(super) unreadable_import: bool,
    /// Whether the options pass through a call that is not a known identity
    /// wrapper, so what was read is a lower bound.
    pub(super) unrecognized_call: bool,
    /// The Federation keys that the argument of an unrecognized call declares.
    pub(super) unrecognized_keys: Vec<FederationKey>,
}

impl ResolvedOptions {
    /// Take what a nested resolution read.
    fn absorb(&mut self, other: Self) {
        for target in other.config.exposed_targets {
            push_unique(&mut self.config.exposed_targets, target);
        }
        for package in other.config.exposed_packages {
            push_unique(&mut self.config.exposed_packages, package);
        }
        for alias in other.config.remote_aliases {
            push_unique(&mut self.config.remote_aliases, alias);
        }
        for package in other.config.shared_packages {
            push_unique(&mut self.config.shared_packages, package);
        }
        for declaration in other.unread {
            push_unique(&mut self.unread, declaration);
        }
        for key in other.declared {
            push_unique(&mut self.declared, key);
        }
        for key in other.unrecognized_keys {
            push_unique(&mut self.unrecognized_keys, key);
        }
        self.unreadable_spread |= other.unreadable_spread;
        self.unreadable_import |= other.unreadable_import;
        self.unrecognized_call |= other.unrecognized_call;
    }
}

/// Read every object literal that an options expression resolves to: an object
/// literal, a top-level binding of the same file, a relative ESM import or
/// `require`, a spread of one of these, `Object.assign(...)` over them, and the
/// argument of a wrapper call.
///
/// Returns `false` when the expression itself does not resolve. A package
/// `require` does not resolve. A relative import or `require` whose target
/// cannot be read resolves, and is recorded as unreadable.
pub(super) fn resolve_options(
    program: &Program<'_>,
    path: &Path,
    expr: &Expression<'_>,
    depth: usize,
    options: &mut ResolvedOptions,
    memo: &mut OptionsMemo,
) -> bool {
    if depth > MAX_OPTIONS_DEPTH {
        return false;
    }
    match unwrap_expression(expr) {
        Expression::ObjectExpression(object) => {
            read_options_object(program, path, object, depth, options, memo);
            true
        }
        Expression::CallExpression(call) if config_parser::is_require_call(call) => {
            resolve_required_options(path, call, depth, options, memo)
        }
        Expression::CallExpression(call) if is_object_assign(&call.callee) => {
            for argument in &call.arguments {
                let resolved = argument.as_expression().is_some_and(|argument| {
                    resolve_options(program, path, argument, depth + 1, options, memo)
                });
                options.unreadable_spread |= !resolved;
            }
            true
        }
        Expression::CallExpression(_) => {
            resolve_wrapped_options(program, path, unwrap_expression(expr), depth, options, memo)
        }
        Expression::Identifier(identifier) => {
            resolve_options_name(program, path, &identifier.name, depth, options, memo)
        }
        _ => false,
    }
}

/// Read the options a wrapper call receives.
///
/// A known identity wrapper passes its argument through unchanged. Any other
/// call can add to or change what it returns, so the object literal passed to
/// it is read as a lower bound and the call is recorded. A call with no
/// readable argument does not resolve. `expr` is the call expression.
fn resolve_wrapped_options(
    program: &Program<'_>,
    path: &Path,
    expr: &Expression<'_>,
    depth: usize,
    options: &mut ResolvedOptions,
    memo: &mut OptionsMemo,
) -> bool {
    let Expression::CallExpression(call) = expr else {
        return false;
    };
    // Resolve the argument on its own, so the keys it declares are known.
    let mut received = ResolvedOptions::default();
    let resolved = call
        .arguments
        .first()
        .and_then(Argument::as_expression)
        .is_some_and(|argument| {
            resolve_options(program, path, argument, depth + 1, &mut received, memo)
        })
        || config_parser::extract_object_from_expression(expr).is_some_and(|object| {
            read_options_object(program, path, object, depth + 1, &mut received, memo);
            true
        });
    if !resolved {
        return false;
    }
    if !is_identity_wrapper(&call.callee) {
        received.unrecognized_call = true;
        received.unrecognized_keys.clone_from(&received.declared);
    }
    options.absorb(received);
    true
}

/// Whether a callee is a known identity wrapper, called by name or as a
/// member.
fn is_identity_wrapper(callee: &Expression<'_>) -> bool {
    let name = match unwrap_expression(callee) {
        Expression::Identifier(identifier) => identifier.name.as_str(),
        Expression::StaticMemberExpression(member) => member.property.name.as_str(),
        _ => return false,
    };
    IDENTITY_WRAPPERS.contains(&name)
}

/// Read the declarations of one options object literal, and follow each spread
/// in it.
pub(super) fn read_options_object(
    program: &Program<'_>,
    path: &Path,
    object: &ObjectExpression<'_>,
    depth: usize,
    options: &mut ResolvedOptions,
    memo: &mut OptionsMemo,
) {
    for key in [
        FederationKey::Exposes,
        FederationKey::Remotes,
        FederationKey::Shared,
    ] {
        if config_parser::property_expr(object, key.name()).is_some() {
            push_unique(&mut options.declared, key);
        }
    }
    read_exposes(object, &mut options.config, &mut options.unread);
    read_remotes(object, &mut options.config, &mut options.unread);
    read_shared(object, &mut options.config);
    for property in &object.properties {
        if let ObjectPropertyKind::SpreadProperty(spread) = property {
            let resolved =
                resolve_options(program, path, &spread.argument, depth + 1, options, memo);
            options.unreadable_spread |= !resolved;
        }
    }
}

/// Resolve a name to its options: a stable top-level binding of the same file
/// first, then a relative ESM import.
///
/// A config can spread one name many times, and each lookup walks the whole
/// program. The result of a name depends only on the file, the name and the
/// depth, so `memo` keeps it and a repeated spread reuses it.
fn resolve_options_name(
    program: &Program<'_>,
    path: &Path,
    name: &str,
    depth: usize,
    options: &mut ResolvedOptions,
    memo: &mut OptionsMemo,
) -> bool {
    let key = (path.to_path_buf(), name.to_string(), depth);
    if let Some((resolved, read)) = memo.get(&key) {
        options.absorb(read.clone());
        return *resolved;
    }
    let mut read = ResolvedOptions::default();
    let resolved = resolve_options_name_uncached(program, path, name, depth, &mut read, memo);
    memo.insert(key, (resolved, read.clone()));
    options.absorb(read);
    resolved
}

fn resolve_options_name_uncached(
    program: &Program<'_>,
    path: &Path,
    name: &str,
    depth: usize,
    options: &mut ResolvedOptions,
    memo: &mut OptionsMemo,
) -> bool {
    if let Some(init) = config_parser::find_stable_binding_init(program, name, false) {
        return resolve_options_init(program, path, init, depth + 1, options, memo);
    }
    let Some((specifier, imported_name)) =
        config_parser::find_relative_import_binding(program, name)
    else {
        return false;
    };
    resolve_module_options(
        path,
        &specifier,
        imported_name.as_deref(),
        depth,
        options,
        memo,
    )
}

/// Record a followed import target that could not be read. The import is
/// still a resolved value: what it holds is unknown, not absent.
fn record_unreadable_import(resolved: bool, options: &mut ResolvedOptions) -> bool {
    options.unreadable_import |= !resolved;
    true
}

/// Resolve the options a relative `require('./x')` names: the value that module
/// exports as a whole. A package `require` does not resolve, and a relative
/// target that cannot be read is recorded.
fn resolve_required_options(
    path: &Path,
    call: &CallExpression<'_>,
    depth: usize,
    options: &mut ResolvedOptions,
    memo: &mut OptionsMemo,
) -> bool {
    let Some(specifier) = config_parser::get_require_source(call)
        .filter(|specifier| config_parser::is_relative_specifier(specifier))
    else {
        return false;
    };
    resolve_module_options(path, &specifier, None, depth, options, memo)
}

/// Read the options a relative sibling module exports: under `export_name`,
/// or as the whole module when `export_name` is `None`. A target that cannot
/// be read is recorded, and the value still counts as resolved.
fn resolve_module_options(
    path: &Path,
    specifier: &str,
    export_name: Option<&str>,
    depth: usize,
    options: &mut ResolvedOptions,
    memo: &mut OptionsMemo,
) -> bool {
    let Some((module_path, source)) = config_parser::resolve_sibling_module(path, specifier) else {
        return record_unreadable_import(false, options);
    };
    let resolved = config_parser::extract_from_source(&source, &module_path, |module| {
        let init = match export_name {
            Some(name) => config_parser::find_exported_init(module, Some(name))?,
            None => config_parser::find_module_export_expression(module)?,
        };
        resolve_options_init(module, &module_path, init, depth + 1, options, memo).then_some(())
    })
    .is_some();
    record_unreadable_import(resolved, options)
}

/// Resolve the value a binding or an export holds. Beyond the shapes of
/// [`resolve_options`], this accepts a function that returns the options.
fn resolve_options_init(
    program: &Program<'_>,
    path: &Path,
    init: &Expression<'_>,
    depth: usize,
    options: &mut ResolvedOptions,
    memo: &mut OptionsMemo,
) -> bool {
    if resolve_options(program, path, init, depth, options, memo) {
        return true;
    }
    let Some(object) = config_parser::extract_object_from_expression(init) else {
        return false;
    };
    read_options_object(program, path, object, depth, options, memo);
    true
}

/// Whether a callee is `Object.assign`.
fn is_object_assign(callee: &Expression<'_>) -> bool {
    matches!(
        unwrap_expression(callee),
        Expression::StaticMemberExpression(member)
            if member.property.name == "assign"
                && matches!(&member.object, Expression::Identifier(object) if object.name == "Object")
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::super::{FederationSites, read};

    #[test]
    fn repeated_self_spreads_of_the_options_resolve_in_bounded_time() {
        // Without a memo, each spread resolves the name again with a walk of
        // the whole program, so 200 spreads cost about 200^3 steps.
        let spreads = vec!["...o"; 200].join(", ");
        let source = format!(
            "const {{ ModuleFederationPlugin }} = require('webpack').container;\n\
             const o = {{ {spreads}, exposes: {{ './a': './src/a.js' }} }};\n\
             module.exports = {{ plugins: [new ModuleFederationPlugin(o)] }};\n"
        );
        let started = std::time::Instant::now();
        let (config, _) = read(
            &source,
            Path::new("webpack.config.js"),
            &FederationSites {
                read_plugin_calls: true,
                read_config_object: false,
            },
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "took {:?}",
            started.elapsed()
        );
        assert_eq!(config.exposed_targets, vec!["./src/a.js"]);
    }
}
