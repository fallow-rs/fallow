//! Argument facts beyond literals: immutable bindings and object literals.
//!
//! A call argument that names a module-scope `const` with a literal
//! initializer, or an enum member, has an identity that does not change
//! between calls. An object literal with only static keys gives the set of
//! option keys that the call passes.

use fallow_types::extract::{
    ArgFact, BindingArg, BindingRoot, ImportInfo, ImportedName, KeyArg, KeyValue,
};
use oxc_ast::ast::{
    BindingPattern, Declaration, ExportDefaultDeclarationKind, Expression, ModuleExportName,
    ObjectExpression, ObjectPropertyKind, Program, PropertyKey, PropertyKind, Statement,
    TSEnumDeclaration, TSEnumMemberName, VariableDeclaration, VariableDeclarationKind,
};
use oxc_semantic::{Scoping, Semantic, SymbolId};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::js_text::JsText as _;

/// The most keys that an object literal argument fact keeps. A larger
/// object is unknown, because a dropped key would look absent.
pub const MAX_OBJECT_KEYS: usize = 32;

/// The longest property key, in bytes, that a fact keeps. An object literal
/// with a longer key is unknown.
pub const MAX_KEY_BYTES: usize = 64;

/// The property names that each object literal inherits from
/// `Object.prototype`. A pattern key with one of these names reads the
/// inherited value when no caller passes the key.
pub const PROTOTYPE_KEYS: [&str; 12] = [
    "__proto__",
    "__defineGetter__",
    "__defineSetter__",
    "__lookupGetter__",
    "__lookupSetter__",
    "constructor",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toLocaleString",
    "toString",
    "valueOf",
];

/// The immutable bindings that an argument can name.
#[derive(Default)]
pub struct ValueScope {
    /// Module-scope `const` bindings with a literal initializer.
    consts: FxHashSet<SymbolId>,
    /// Module-scope enums, with the names of their members.
    enums: FxHashMap<SymbolId, FxHashSet<Box<str>>>,
    /// Admitted import bindings: the import index, and `true` for a
    /// namespace import.
    imports: FxHashMap<SymbolId, (u32, bool)>,
}

/// The value scope of a program, and the local names of the exported
/// literal constants.
pub fn value_scope(
    semantic: &Semantic<'_>,
    program: &Program<'_>,
    imports: &[ImportInfo],
) -> (ValueScope, Vec<Box<str>>) {
    let mut scope = ValueScope::default();
    let mut consts: Vec<(SymbolId, &str, bool)> = Vec::new();
    let mut exported_names: FxHashSet<&str> = FxHashSet::default();
    for statement in &program.body {
        match statement {
            Statement::VariableDeclaration(declaration) => {
                push_consts(semantic.scoping(), &mut consts, declaration, false);
            }
            Statement::TSEnumDeclaration(declaration) => push_enum(&mut scope, declaration),
            Statement::ExportDeclaration(export) => match &export.declaration {
                Declaration::VariableDeclaration(declaration) => {
                    push_consts(semantic.scoping(), &mut consts, declaration, true);
                }
                Declaration::TSEnumDeclaration(declaration) => push_enum(&mut scope, declaration),
                _ => {}
            },
            Statement::ExportNamedDeclaration(export) if !export.export_kind.is_type() => {
                for specifier in &export.specifiers {
                    if let ModuleExportName::IdentifierReference(local) = &specifier.local {
                        exported_names.insert(local.name.as_str());
                    }
                }
            }
            Statement::ExportDefaultDeclaration(export) => {
                if let ExportDefaultDeclarationKind::Identifier(local) = &export.declaration {
                    exported_names.insert(local.name.as_str());
                }
            }
            _ => {}
        }
    }
    let mut exported = Vec::new();
    for (symbol, name, direct_export) in consts {
        scope.consts.insert(symbol);
        if direct_export || exported_names.contains(name) {
            exported.push(name.into());
        }
    }
    for (index, import) in imports.iter().enumerate() {
        let Ok(index) = u32::try_from(index) else {
            break;
        };
        if let Some(symbol) = crate::binding_references::admitted_import_symbol(semantic, import) {
            let namespace = import.imported_name == ImportedName::Namespace;
            scope.imports.insert(symbol, (index, namespace));
        }
    }
    (scope, exported)
}

fn push_consts<'b>(
    scoping: &Scoping,
    consts: &mut Vec<(SymbolId, &'b str, bool)>,
    declaration: &'b VariableDeclaration<'_>,
    exported: bool,
) {
    if declaration.declare || declaration.kind != VariableDeclarationKind::Const {
        return;
    }
    for declarator in &declaration.declarations {
        let BindingPattern::BindingIdentifier(id) = &declarator.id else {
            continue;
        };
        let Some(init) = &declarator.init else {
            continue;
        };
        let init = super::unwrap_transparent(init);
        if super::literal_value(scoping, init).is_none() {
            continue;
        }
        if let Some(symbol) = id.symbol_id.get() {
            consts.push((symbol, id.name.as_str(), exported));
        }
    }
}

fn push_enum(scope: &mut ValueScope, declaration: &TSEnumDeclaration<'_>) {
    let Some(symbol) = declaration.id.symbol_id.get() else {
        return;
    };
    let members = scope.enums.entry(symbol).or_default();
    for member in &declaration.body.members {
        match &member.id {
            TSEnumMemberName::Identifier(name) => {
                members.insert(name.name.as_str().into());
            }
            TSEnumMemberName::String(name) => {
                members.insert(name.value.text().into());
            }
            TSEnumMemberName::ComputedString(_) | TSEnumMemberName::ComputedTemplateString(_) => {}
        }
    }
}

impl ValueScope {
    /// The binding fact of `expr`: an identifier, or a static member chain
    /// over an identifier, that names an immutable binding.
    pub fn binding(&self, scoping: &Scoping, expr: &Expression<'_>) -> Option<BindingArg> {
        let mut segments: Vec<&str> = Vec::new();
        let mut current = super::unwrap_transparent(expr);
        let root = loop {
            match current {
                Expression::Identifier(identifier) => break identifier,
                Expression::StaticMemberExpression(member) if !member.optional => {
                    segments.push(member.property.name.as_str());
                    current = super::unwrap_transparent(&member.object);
                }
                _ => return None,
            }
        };
        // Two segments are the most that a binding needs: a namespace
        // import, its export, and an enum member.
        if segments.len() > 2 {
            return None;
        }
        segments.reverse();
        let symbol = scoping
            .get_reference(root.reference_id.get()?)
            .symbol_id()?;
        if let Some(&(index, namespace)) = self.imports.get(&symbol) {
            let fits = if namespace {
                !segments.is_empty()
            } else {
                segments.len() <= 1
            };
            return fits.then(|| BindingArg {
                root: BindingRoot::Import(index),
                path: segments.join(".").into(),
            });
        }
        let name = root.name.as_str();
        match segments.as_slice() {
            [] if self.consts.contains(&symbol) => Some(BindingArg {
                root: BindingRoot::Local,
                path: name.into(),
            }),
            [member] if self.enums.get(&symbol).is_some_and(|m| m.contains(*member)) => {
                Some(BindingArg {
                    root: BindingRoot::Local,
                    path: format!("{name}.{member}").into(),
                })
            }
            _ => None,
        }
    }
}

/// The fact of an object literal argument. `pass_through` holds the key
/// bindings of the same parameter position of a self-call.
pub fn object_arg(
    scoping: &Scoping,
    values: &ValueScope,
    object: &ObjectExpression<'_>,
    pass_through: &[(Box<str>, SymbolId)],
) -> ArgFact {
    if object.properties.len() > MAX_OBJECT_KEYS {
        return ArgFact::Unknown;
    }
    let mut keys: Vec<KeyArg> = Vec::with_capacity(object.properties.len());
    for property in &object.properties {
        let ObjectPropertyKind::ObjectProperty(property) = property else {
            return ArgFact::Unknown;
        };
        if property.computed {
            return ArgFact::Unknown;
        }
        let key = match &property.key {
            PropertyKey::StaticIdentifier(id) => id.name.as_str().to_string(),
            PropertyKey::StringLiteral(literal) => literal.value.text().to_string(),
            _ => return ArgFact::Unknown,
        };
        // `__proto__: x` sets the prototype, so the object can inherit any
        // key.
        if key == "__proto__" || key.len() > MAX_KEY_BYTES {
            return ArgFact::Unknown;
        }
        let value = if property.kind == PropertyKind::Init && !property.method {
            key_value(scoping, values, &key, &property.value, pass_through)
        } else {
            KeyValue::Unknown
        };
        match keys.iter_mut().find(|existing| *existing.key == *key) {
            Some(existing) => existing.value = value,
            None => keys.push(KeyArg {
                key: key.into(),
                value,
            }),
        }
    }
    ArgFact::Object(keys)
}

fn key_value(
    scoping: &Scoping,
    values: &ValueScope,
    key: &str,
    value: &Expression<'_>,
    pass_through: &[(Box<str>, SymbolId)],
) -> KeyValue {
    let value = super::unwrap_transparent(value);
    if let Some(literal) = super::literal_value(scoping, value) {
        return KeyValue::Literal(literal);
    }
    if let Expression::Identifier(identifier) = value
        && let Some(symbol) = identifier
            .reference_id
            .get()
            .and_then(|reference| scoping.get_reference(reference).symbol_id())
        && pass_through
            .iter()
            .any(|(name, binding)| **name == *key && *binding == symbol)
    {
        return KeyValue::PassThrough;
    }
    values
        .binding(scoping, value)
        .map_or(KeyValue::Unknown, KeyValue::Binding)
}
