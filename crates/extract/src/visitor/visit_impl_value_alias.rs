//! Value aliases of a binding: `const Alias = Color` and the class field
//! `levels = Level`. A member read through the alias is a member read of the
//! binding (issue #4578).

use oxc_ast::ast::{BindingPattern, Expression, VariableDeclarationKind, VariableDeclarator};
use rustc_hash::FxHashSet;

use super::super::{BindingTarget, ModuleInfoExtractor};

impl ModuleInfoExtractor {
    /// Record a module-scope `const` alias. A reassigned `let` can name
    /// another binding at the point of the read. The binding map is
    /// module-flat, so a nested `const` with the name of an outer binding
    /// would take over the reads of the outer binding.
    pub(in crate::visitor) fn record_declarator_value_alias(
        &mut self,
        declarator: &VariableDeclarator<'_>,
        kind: VariableDeclarationKind,
        init: &Expression<'_>,
    ) {
        if let BindingPattern::BindingIdentifier(id) = &declarator.id
            && kind == VariableDeclarationKind::Const
            && self.is_module_scope()
            && self.record_value_alias(id.name.to_string(), init)
        {
            self.shadowed_reads
                .module_aliases
                .insert(id.name.to_string());
        }
    }

    /// Bind `binding` to the binding that a value alias names
    /// (`const Alias = Color`, the class field `levels = Level`), so
    /// `Alias.Red` also records `Color.Red` (issue #4578). A source with a
    /// known class target passes that target on. A namespace object keeps its
    /// whole-object use (issue #2377) and gets no alias.
    ///
    /// An alias never replaces a binding that `binding` already has: a type
    /// annotation or a `new Class()` is the more precise answer, and a conflict
    /// would make the module-flat binding ambiguous for the reads that it
    /// resolves today.
    ///
    /// Returns whether `binding` now resolves through the alias.
    pub(in crate::visitor) fn record_value_alias(
        &mut self,
        binding: String,
        value: &Expression<'_>,
    ) -> bool {
        if self.binding_target_names.contains_key(&binding) {
            return false;
        }
        let Some(source) = value_alias_source(value) else {
            return false;
        };
        if self.carries_namespace_object(source) {
            return false;
        }
        let target = match self.resolve_bound_object_name(source) {
            Some(BindingTarget::Class(target)) => target,
            Some(BindingTarget::FactoryCall { .. } | BindingTarget::Ambiguous) => return false,
            None => source.to_string(),
        };
        self.insert_class_binding_target_if_absent(binding, target);
        true
    }
}

/// The member reads that must not resolve through the module-flat class
/// bindings, by index into `member_accesses`.
#[derive(Debug, Default)]
pub(in crate::visitor) struct ShadowedReads {
    /// Reads whose receiver root is a nested binding that provably holds no
    /// class instance.
    pub(in crate::visitor) class_roots: FxHashSet<usize>,
    /// Reads whose receiver root is a name that a nested scope declares, such
    /// as a parameter or a nested `const`.
    pub(in crate::visitor) nested_roots: FxHashSet<usize>,
    /// Module-scope `const` aliases of a binding (`const Alias = Color`).
    pub(in crate::visitor) module_aliases: FxHashSet<String>,
}

impl ShadowedReads {
    /// Whether the root of `object` is a module-scope `const` alias.
    pub(in crate::visitor) fn is_module_alias(&self, root: &str) -> bool {
        self.module_aliases.contains(root)
    }

    /// Whether the read at `index` of `object` skips the class bindings. A
    /// nested declaration with the name of a module alias holds another
    /// value, so its reads do not resolve through the alias.
    pub(in crate::visitor) fn skips(&self, index: usize, object: &str) -> bool {
        let root = object.split_once('.').map_or(object, |(root, _)| root);
        self.class_roots.contains(&index)
            || (self.nested_roots.contains(&index) && self.is_module_alias(root))
    }
}

/// The binding that a value alias names: the bare identifier of
/// `const Alias = Color` or of the class field `levels = Level`.
///
/// The alias holds the same object as the binding, so a member read through
/// the alias is a member read of the binding (issue #4578). The global value
/// names `undefined`, `NaN` and `Infinity` are identifiers too, but they hold
/// no object with members.
pub(in crate::visitor) fn value_alias_source<'b>(expr: &'b Expression<'_>) -> Option<&'b str> {
    let Expression::Identifier(ident) = unwrap_static_expr(expr) else {
        return None;
    };
    let name = ident.name.as_str();
    (!matches!(name, "undefined" | "NaN" | "Infinity")).then_some(name)
}

/// Strip the parentheses and the TypeScript wrappers that do not change the
/// runtime value.
pub(in crate::visitor) fn unwrap_static_expr<'a, 'b>(
    mut expr: &'b Expression<'a>,
) -> &'b Expression<'a> {
    loop {
        match expr {
            Expression::ParenthesizedExpression(paren) => expr = &paren.expression,
            Expression::TSAsExpression(ts_as) => expr = &ts_as.expression,
            Expression::TSSatisfiesExpression(ts_sat) => expr = &ts_sat.expression,
            Expression::TSNonNullExpression(ts_non_null) => expr = &ts_non_null.expression,
            _ => return expr,
        }
    }
}

/// Whether `name` is a built-in constructor, not a project class.
pub(in crate::visitor) fn is_builtin_constructor(name: &str) -> bool {
    matches!(
        name,
        "Array"
            | "ArrayBuffer"
            | "Blob"
            | "Boolean"
            | "DataView"
            | "Date"
            | "Error"
            | "EvalError"
            | "Event"
            | "Float32Array"
            | "Float64Array"
            | "FormData"
            | "Headers"
            | "Int8Array"
            | "Int16Array"
            | "Int32Array"
            | "Map"
            | "Number"
            | "Object"
            | "Promise"
            | "Proxy"
            | "RangeError"
            | "ReferenceError"
            | "RegExp"
            | "Request"
            | "Response"
            | "Set"
            | "SharedArrayBuffer"
            | "String"
            | "SyntaxError"
            | "TypeError"
            | "URIError"
            | "URL"
            | "URLSearchParams"
            | "Uint8Array"
            | "Uint8ClampedArray"
            | "Uint16Array"
            | "Uint32Array"
            | "WeakMap"
            | "WeakRef"
            | "WeakSet"
            | "Worker"
            | "AbortController"
            | "ReadableStream"
            | "WritableStream"
            | "TransformStream"
            | "TextEncoder"
            | "TextDecoder"
            | "MutationObserver"
            | "IntersectionObserver"
            | "ResizeObserver"
            | "PerformanceObserver"
            | "MessageChannel"
            | "BroadcastChannel"
            | "WebSocket"
            | "XMLHttpRequest"
            | "EventEmitter"
            | "Buffer"
    )
}
