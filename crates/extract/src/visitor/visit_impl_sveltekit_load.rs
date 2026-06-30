//! SvelteKit `load()` return-key harvesting.

use oxc_ast::ast::{
    BindingPattern, Declaration, Expression, FunctionBody, ObjectPropertyKind, TSType, TSTypeName,
};
use oxc_span::GetSpan;

use super::{
    ModuleInfoExtractor, count_returns_in_statements, extract_arrow_return_expr,
    extract_function_body_final_return_expr, unwrap_paren_expr,
};

impl ModuleInfoExtractor {
    /// Harvest the SvelteKit `load()` return-object keys from an
    /// `export const load = ...` / `export [async] function load` declaration.
    /// The harvest is loose here (it fires for ANY exported `load`); `parse.rs`
    /// clears the result for non-`+page.{ts,server.ts,js,server.js}` files and
    /// the analyze-layer `@sveltejs/kit` gate is the activation boundary.
    /// Abstains (sets `has_unharvestable_load`) on any unsafe shape, mirroring
    /// the Pinia setup-store harvest abstain.
    pub(super) fn try_harvest_load_export(&mut self, declaration: &Declaration<'_>) {
        match declaration {
            Declaration::FunctionDeclaration(function) => {
                if function.id.as_ref().is_none_or(|id| id.name != "load") {
                    return;
                }
                let Some(body) = function.body.as_ref() else {
                    // A bodyless overload signature carries no keys; ignore.
                    return;
                };
                match load_terminal_return_expr(body) {
                    Ok(Some(returned)) => self.harvest_load_terminal(returned),
                    Ok(None) => {}
                    Err(()) => self.has_unharvestable_load = true,
                }
            }
            Declaration::VariableDeclaration(var) => {
                for declarator in &var.declarations {
                    // The binding must be named `load`. A `: PageLoad` annotation
                    // (S4) only matters in addition to the name; SvelteKit always
                    // exports the function under the name `load`.
                    let is_load_binding = matches!(
                        &declarator.id,
                        BindingPattern::BindingIdentifier(id) if id.name == "load"
                    );
                    if !is_load_binding {
                        continue;
                    }
                    // S4: a `: PageLoad` annotation is recognized but not
                    // required; harvest proceeds either way.
                    let _has_load_annotation = declarator
                        .type_annotation
                        .as_deref()
                        .and_then(|ann| ts_type_reference_base_name(&ann.type_annotation))
                        .is_some_and(|name| is_sveltekit_load_type_name(&name));
                    let Some(init) = declarator.init.as_ref() else {
                        continue;
                    };
                    self.harvest_load_init(init);
                }
            }
            _ => {}
        }
    }

    /// Harvest a `load` binding initializer: an arrow / function expression
    /// (optionally wrapped in `satisfies PageLoad` / a TS `as` cast). Any other
    /// shape (a wrapped factory `load = wrap(...)`, an identifier) abstains.
    fn harvest_load_init(&mut self, init: &Expression<'_>) {
        // Peel every TS-cast / satisfies / parenthesis wrapper layer (any order),
        // so `(async () => ({...})) satisfies PageLoad` reaches the arrow.
        let mut unwrapped = init;
        loop {
            match unwrapped {
                Expression::TSSatisfiesExpression(sat) => unwrapped = &sat.expression,
                Expression::TSAsExpression(as_expr) => unwrapped = &as_expr.expression,
                Expression::ParenthesizedExpression(paren) => unwrapped = &paren.expression,
                _ => break,
            }
        }
        match unwrapped {
            Expression::ArrowFunctionExpression(arrow) => {
                if arrow.expression {
                    // `load = () => ({ ... })` single-expression body.
                    match extract_arrow_return_expr(arrow) {
                        Some(returned) => self.harvest_load_terminal(returned),
                        None => self.has_unharvestable_load = true,
                    }
                    return;
                }
                match load_terminal_return_expr(&arrow.body) {
                    Ok(Some(returned)) => self.harvest_load_terminal(returned),
                    Ok(None) => {}
                    Err(()) => self.has_unharvestable_load = true,
                }
            }
            Expression::FunctionExpression(func) => match func.body.as_ref() {
                Some(body) => match load_terminal_return_expr(body) {
                    Ok(Some(returned)) => self.harvest_load_terminal(returned),
                    Ok(None) => {}
                    Err(()) => self.has_unharvestable_load = true,
                },
                None => self.has_unharvestable_load = true,
            },
            // `export const load = wrappedLoad(...)` / a bare identifier: the
            // terminal object is not a direct literal here, so abstain.
            _ => self.has_unharvestable_load = true,
        }
    }

    /// Harvest the keys from a terminal return expression, or abstain.
    fn harvest_load_terminal(&mut self, returned: &Expression<'_>) {
        match harvest_load_return_keys(returned) {
            Ok(keys) => self.load_return_keys.extend(keys),
            Err(()) => self.has_unharvestable_load = true,
        }
    }
}

/// Whether a TS type annotation names a SvelteKit load type (`PageLoad`,
/// `PageServerLoad`, `LayoutLoad`, `LayoutServerLoad`), used to recognize a
/// `load` declared via `: PageLoad` annotation or `satisfies PageLoad`. The
/// type may be generic (`PageLoad<{ ... }>`); only the base name is checked.
fn is_sveltekit_load_type_name(name: &str) -> bool {
    matches!(
        name,
        "PageLoad" | "PageServerLoad" | "LayoutLoad" | "LayoutServerLoad"
    )
}

/// Extract the base name of a `TSTypeReference` (`PageLoad<X>` -> `PageLoad`).
fn ts_type_reference_base_name(ty: &TSType<'_>) -> Option<String> {
    let TSType::TSTypeReference(type_ref) = ty else {
        return None;
    };
    match &type_ref.type_name {
        TSTypeName::IdentifierReference(id) => Some(id.name.to_string()),
        TSTypeName::QualifiedName(qualified) => Some(qualified.right.name.to_string()),
        TSTypeName::ThisExpression(_) => None,
    }
}

/// Harvest the load() return-object keys from a terminal return object literal.
/// Returns `Ok(keys)` with the property key names + spans, or `Err(())` to
/// signal an abstain (spread, non-object/non-literal return, or a computed key).
fn harvest_load_return_keys(
    returned: &Expression<'_>,
) -> Result<Vec<fallow_types::extract::LoadReturnKey>, ()> {
    let Expression::ObjectExpression(obj) = unwrap_paren_expr(returned) else {
        // A non-object terminal return (`return data`, `return makeData()`)
        // cannot be key-harvested: abstain.
        return Err(());
    };
    let mut keys = Vec::new();
    for prop in &obj.properties {
        match prop {
            // A spread (`return { ...base, x }`) hides keys: abstain entirely.
            ObjectPropertyKind::SpreadProperty(_) => return Err(()),
            ObjectPropertyKind::ObjectProperty(prop) => {
                // A computed key (`return { [k]: v }`) is unknowable: abstain.
                if prop.computed {
                    return Err(());
                }
                let Some(name) = prop.key.static_name() else {
                    return Err(());
                };
                let span = prop.key.span();
                keys.push(fallow_types::extract::LoadReturnKey {
                    name: name.to_string(),
                    span_start: span.start,
                    span_end: span.end,
                });
            }
        }
    }
    Ok(keys)
}

/// The terminal return object of a function/arrow `load` body, with the
/// multi-return abstain applied. `Ok(Some(obj))` = a single terminal-return
/// object literal; `Ok(None)` = no return (an empty/void body, no keys to
/// harvest, no abstain); `Err(())` = abstain (>1 return).
fn load_terminal_return_expr<'a, 'b>(
    body: &'b FunctionBody<'a>,
) -> Result<Option<&'b Expression<'a>>, ()> {
    if count_returns_in_statements(&body.statements) > 1 {
        return Err(());
    }
    Ok(extract_function_body_final_return_expr(body))
}
