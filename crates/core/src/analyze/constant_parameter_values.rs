//! What each call passes for one parameter or one option key, and when two
//! calls pass the same value.
//!
//! Two arguments are the same when both are the same literal, or both name
//! the same immutable binding: one `const` with a literal initializer, or
//! one enum member, in the module that declares it. An object literal, an
//! array literal and a folded expression are never the same as another
//! argument.

use fallow_types::extract::{
    ArgAt, ArgFact, BindingArg, BindingRoot, CallSite, ImportedName, KeyValue, LiteralValue,
    MemberKind, OptionKeyFact, ParamFact,
};

use super::{Call, Gate, export_local_name, facts, import_export_name};
use crate::discover::FileId;
use crate::graph::{EffectiveExportResolution, ExportNamespace};
use crate::results::{ConstantParameterReason, ConstantParameterValue, ConstantParameterValueKind};

/// An immutable binding in the module that declares it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BindingKey<'a> {
    file: FileId,
    /// The local name of the binding in its module.
    name: &'a str,
    /// The enum member, empty for a constant.
    member: &'a str,
}

/// A value that two calls can share.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Value<'a> {
    Literal(&'a LiteralValue),
    Binding(BindingKey<'a>),
}

impl Value<'_> {
    pub(super) fn wire(self) -> ConstantParameterValue {
        match self {
            Self::Literal(literal) => ConstantParameterValue::from_literal(literal),
            Self::Binding(key) => ConstantParameterValue {
                kind: ConstantParameterValueKind::Binding,
                text: if key.member.is_empty() {
                    key.name.to_string()
                } else {
                    format!("{}.{}", key.name, key.member)
                },
            },
        }
    }
}

/// What one call passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Observed<'a> {
    Omitted,
    Value(Value<'a>),
    Unknown,
}

/// The reason and the value when each call passes the same thing. `optional`
/// is `true` when an omitted value is valid.
fn verdict<'a>(
    optional: bool,
    mut observed: impl Iterator<Item = Observed<'a>>,
) -> Option<(ConstantParameterReason, Option<Value<'a>>)> {
    match observed.next()? {
        Observed::Omitted => (optional && observed.all(|seen| seen == Observed::Omitted))
            .then_some((ConstantParameterReason::NeverPassed, None)),
        Observed::Value(first) => observed
            .all(|seen| seen == Observed::Value(first))
            .then_some((ConstantParameterReason::IdenticalArgument, Some(first))),
        Observed::Unknown => None,
    }
}

/// `true` when a self-call passes parameter `index` on unchanged.
pub(super) fn is_pass_through(site: &CallSite, index: u32) -> bool {
    matches!(site.arg_at(index), ArgAt::Fact(ArgFact::Param(param)) if *param == index)
}

/// `true` when a self-call passes option `key` of parameter `index` on
/// unchanged.
pub(super) fn is_key_pass_through(site: &CallSite, index: u32, key: &str) -> bool {
    matches!(
        site.arg_at(index),
        ArgAt::Fact(ArgFact::Object(keys))
            if keys.iter().any(|arg| *arg.key == *key && arg.value == KeyValue::PassThrough)
    )
}

impl<'a> Gate<'a> {
    /// The verdict for an identifier parameter.
    pub(super) fn parameter_verdict(
        &self,
        param: &ParamFact,
        index: u32,
        calls: &[Call<'a>],
    ) -> Option<(ConstantParameterReason, Option<Value<'a>>)> {
        let optional = param.optional || param.has_default;
        verdict(
            optional,
            calls.iter().map(|call| self.observe(*call, index)),
        )
    }

    /// What `call` passes for the identifier parameter at `index`.
    pub(super) fn observe(&self, call: Call<'a>, index: u32) -> Observed<'a> {
        match call.site.arg_at(index) {
            ArgAt::Omitted => Observed::Omitted,
            ArgAt::Fact(ArgFact::Literal(literal)) => Observed::Value(Value::Literal(literal)),
            ArgAt::Fact(ArgFact::Binding(binding)) => self.binding_value(call.file, binding),
            ArgAt::Fact(ArgFact::Param(_) | ArgFact::Object(_) | ArgFact::Unknown)
            | ArgAt::Unknown => Observed::Unknown,
        }
    }

    /// The verdict for one option key of an object-pattern parameter.
    ///
    /// The extractor records keys only when the parameter has no default or
    /// the default `{}`, so an omitted parameter with that default, and an
    /// `undefined` argument, set no key. An omitted parameter with no default
    /// throws at the destructuring, so it is unknown.
    pub(super) fn key_verdict(
        &self,
        param: &ParamFact,
        key: &OptionKeyFact,
        index: u32,
        calls: &[Call<'a>],
    ) -> Option<(ConstantParameterReason, Option<Value<'a>>)> {
        let absent = if param.has_default {
            Observed::Omitted
        } else {
            Observed::Unknown
        };
        verdict(
            true,
            calls.iter().map(|call| match call.site.arg_at(index) {
                ArgAt::Omitted | ArgAt::Fact(ArgFact::Literal(LiteralValue::Undefined)) => absent,
                ArgAt::Fact(ArgFact::Object(keys)) => {
                    match keys.iter().find(|arg| *arg.key == *key.key) {
                        None => Observed::Omitted,
                        Some(arg) => match &arg.value {
                            KeyValue::Literal(literal) => Observed::Value(Value::Literal(literal)),
                            KeyValue::Binding(binding) => self.binding_value(call.file, binding),
                            KeyValue::PassThrough | KeyValue::Unknown => Observed::Unknown,
                        },
                    }
                }
                ArgAt::Fact(_) | ArgAt::Unknown => Observed::Unknown,
            }),
        )
    }

    fn binding_value(&self, caller: FileId, binding: &'a BindingArg) -> Observed<'a> {
        self.binding_key(caller, binding)
            .map_or(Observed::Unknown, |key| {
                Observed::Value(Value::Binding(key))
            })
    }

    /// The declaring module and name of the binding that an argument names,
    /// when it is a `const` with a literal initializer or an enum member.
    fn binding_key(&self, caller: FileId, binding: &'a BindingArg) -> Option<BindingKey<'a>> {
        let path = &*binding.path;
        let index = match binding.root {
            BindingRoot::Local => {
                let (name, member) = path.split_once('.').unwrap_or((path, ""));
                return Some(BindingKey {
                    file: caller,
                    name,
                    member,
                });
            }
            BindingRoot::Import(index) => index as usize,
        };
        let module = self.modules.get(&caller)?;
        let import = module.imports.get(index)?;
        let (name, member) = match &import.imported_name {
            ImportedName::Namespace => path.split_once('.').unwrap_or((path, "")),
            other => (import_export_name(other)?, path),
        };
        if name.is_empty() || member.contains('.') {
            return None;
        }
        let target = self
            .resolved_import(module, index)?
            .target
            .internal_file_id()?;
        let graph = self.input.graph;
        let EffectiveExportResolution::Unique(export_binding) =
            graph.resolve_export(target, name, ExportNamespace::Value)
        else {
            return None;
        };
        if export_binding.namespace_source().is_some() {
            return None;
        }
        let origin = graph.export_binding_origin(export_binding)?;
        let file = export_binding.origin_file();
        let origin_module = *self.modules.get(&file)?;
        let symbol = origin.export();
        let export = origin_module
            .exports
            .iter()
            .find(|export| export.name == symbol.name && export.span.start == symbol.span.start)?;
        if export.is_type_only {
            return None;
        }
        let local = export_local_name(export)?;
        let known = if member.is_empty() {
            facts(origin_module)?
                .literal_consts
                .iter()
                .any(|name| **name == *local)
        } else {
            export
                .members
                .iter()
                .any(|m| m.kind == MemberKind::EnumMember && m.name == member)
        };
        known.then_some(BindingKey {
            file,
            name: local,
            member,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRUE: LiteralValue = LiteralValue::Boolean(true);
    const FALSE: LiteralValue = LiteralValue::Boolean(false);

    fn binding(name: &'static str) -> Value<'static> {
        Value::Binding(BindingKey {
            file: FileId(0),
            name,
            member: "",
        })
    }

    #[test]
    fn the_same_literal_or_binding_is_identical() {
        let same = [Observed::Value(Value::Literal(&TRUE)); 2];
        assert_eq!(
            verdict(false, same.into_iter()),
            Some((
                ConstantParameterReason::IdenticalArgument,
                Some(Value::Literal(&TRUE))
            ))
        );
        let bound = [Observed::Value(binding("A")), Observed::Value(binding("A"))];
        assert!(verdict(false, bound.into_iter()).is_some());
    }

    #[test]
    fn different_values_or_an_unknown_give_no_verdict() {
        let literals = [
            Observed::Value(Value::Literal(&TRUE)),
            Observed::Value(Value::Literal(&FALSE)),
        ];
        assert_eq!(verdict(false, literals.into_iter()), None);
        let bindings = [Observed::Value(binding("A")), Observed::Value(binding("B"))];
        assert_eq!(verdict(false, bindings.into_iter()), None);
        let unknown = [Observed::Value(binding("A")), Observed::Unknown];
        assert_eq!(verdict(false, unknown.into_iter()), None);
        let mixed = [Observed::Omitted, Observed::Value(binding("A"))];
        assert_eq!(verdict(true, mixed.into_iter()), None);
    }

    #[test]
    fn omitted_values_are_never_passed_only_when_optional() {
        let omitted = [Observed::Omitted; 2];
        assert_eq!(verdict(false, omitted.into_iter()), None);
        assert_eq!(
            verdict(true, omitted.into_iter()),
            Some((ConstantParameterReason::NeverPassed, None))
        );
    }
}
