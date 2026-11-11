//! The candidates of the rule that need type evidence.
//!
//! The static gate selects them, and the type-aware pass checks them:
//!
//! - A parameter with a default where each call passes an argument. When no
//!   argument can be `undefined`, the default never applies. A literal
//!   argument proves itself, so only the other arguments need a type.
//! - An exported class whose module passes the module-level gate. The
//!   companion finds the calls of its methods through the type of the
//!   receiver.

use fallow_types::extract::{
    ArgAt, ArgFact, ExportInfo, FunctionContract, LiteralValue, MemberKind, ModuleInfo, ParamFact,
    ParamKind,
};

use super::values::{Observed, Value};
use super::{Abstain, Call, FnId, Gate, MIN_CALL_SITES, Subject, export_local_name, facts, values};
use crate::discover::FileId;
use crate::results::{
    CandidateArgument, ConstantParameterCandidates, ConstantParameterEvidence,
    ConstantParameterReason, MethodClassCandidate, UnusedDefaultCandidate,
    constant_parameter_explanation,
};

/// An exported class with at least one method.
pub(super) struct MethodClass<'a> {
    pub local: &'a str,
    pub export: &'a ExportInfo,
}

fn has_method(export: &ExportInfo) -> bool {
    !export.is_type_only
        && export
            .members
            .iter()
            .any(|member| member.kind == MemberKind::ClassMethod)
}

/// Each exported class of a module with a method, once for each local name.
pub(super) fn method_classes(module: &ModuleInfo) -> impl Iterator<Item = MethodClass<'_>> {
    let mut seen = rustc_hash::FxHashSet::default();
    module.exports.iter().filter_map(move |export| {
        if !has_method(export) {
            return None;
        }
        let local = export_local_name(export)?;
        seen.insert(local).then_some(MethodClass { local, export })
    })
}

impl Gate<'_> {
    /// The candidates of the run that need type evidence.
    pub(super) fn type_candidates(&self) -> ConstantParameterCandidates {
        ConstantParameterCandidates {
            defaults: self.default_candidates(),
            classes: self.class_candidates(),
            files: self
                .input
                .graph
                .modules
                .iter()
                .map(|node| node.path.clone())
                .collect(),
        }
    }

    fn path_of(&self, file: FileId) -> std::path::PathBuf {
        self.input
            .graph
            .modules
            .get(file.0 as usize)
            .map(|node| node.path.clone())
            .unwrap_or_default()
    }

    fn reachable(&self, file: FileId) -> bool {
        self.input
            .graph
            .modules
            .get(file.0 as usize)
            .is_some_and(|node| node.is_runtime_reachable())
    }

    fn default_candidates(&self) -> Vec<UnusedDefaultCandidate> {
        let mut out = Vec::new();
        for module in self.input.modules {
            let Some(facts) = facts(module) else {
                continue;
            };
            if !self.reachable(module.file_id) {
                continue;
            }
            for (index, contract) in facts.declarations.iter().enumerate() {
                let Ok(index) = u32::try_from(index) else {
                    break;
                };
                let id: FnId = (module.file_id, index);
                if self.blocked.contains_key(&id) {
                    continue;
                }
                let calls = self.calls.get(&id).map_or(&[][..], Vec::as_slice);
                if calls.len() < MIN_CALL_SITES {
                    continue;
                }
                for (at, param) in contract.params.iter().enumerate() {
                    let Ok(at) = u32::try_from(at) else {
                        break;
                    };
                    out.extend(self.default_candidate(module, contract, param, at, calls));
                }
            }
        }
        out
    }

    fn default_candidate(
        &self,
        module: &ModuleInfo,
        contract: &FunctionContract,
        param: &ParamFact,
        at: u32,
        calls: &[Call<'_>],
    ) -> Option<UnusedDefaultCandidate> {
        if !param.has_default || param.is_rest || param.written {
            return None;
        }
        let (ParamKind::Identifier, Some(name)) = (param.kind, param.name.as_deref()) else {
            return None;
        };
        let counted: Vec<Call<'_>> = calls
            .iter()
            .copied()
            .filter(|call| !values::is_pass_through(call.site, at))
            .collect();
        if counted.len() < MIN_CALL_SITES {
            return None;
        }
        let mut arguments = Vec::new();
        for call in &counted {
            match self.observe(*call, at) {
                Observed::Omitted | Observed::Value(Value::Literal(LiteralValue::Undefined)) => {
                    return None;
                }
                Observed::Value(Value::Literal(_)) => {}
                Observed::Value(Value::Binding(_)) | Observed::Unknown => {
                    // An object literal is never `undefined`.
                    if !matches!(call.site.arg_at(at), ArgAt::Fact(ArgFact::Object(_))) {
                        arguments.push(CandidateArgument {
                            path: self.path_of(call.file),
                            call_start: call.site.span_start,
                            index: at,
                        });
                    }
                }
            }
        }
        // When each argument is a literal, the default needs no type evidence,
        // and type-aware mode reports only what needs it.
        if arguments.is_empty() {
            return None;
        }
        if self.suppressed(module.file_id, param.span_start)
            || self.suppressed(module.file_id, contract.span_start)
        {
            return None;
        }
        let subject = Subject {
            index: at,
            name,
            option_key: None,
            optional: param.optional,
            span_start: param.span_start,
            has_default: param.has_default,
            condition_reads: param.condition_reads,
        };
        let mut finding = self.finding(
            module,
            contract,
            &subject,
            (ConstantParameterReason::UnusedDefault, None),
            &counted,
        );
        finding.evidence = Some(ConstantParameterEvidence::TypeAware);
        finding.explanation = constant_parameter_explanation(
            ConstantParameterReason::UnusedDefault,
            false,
            finding.evidence,
        );
        Some(UnusedDefaultCandidate { finding, arguments })
    }

    fn class_candidates(&self) -> Vec<MethodClassCandidate> {
        let mut out = Vec::new();
        for module in self.input.modules {
            if !self.reachable(module.file_id) {
                continue;
            }
            let offset = facts(module).map_or(0, |facts| facts.declarations.len());
            for (at, class) in method_classes(module).enumerate() {
                let Ok(index) = u32::try_from(offset + at) else {
                    break;
                };
                let reasons = self
                    .reasons
                    .get(&(module.file_id, index))
                    .copied()
                    .unwrap_or_default();
                // A value reference to a class, such as `new Service()` or a
                // registration, leaves each call of a method visible through
                // the type of the receiver.
                if Abstain::ALL
                    .into_iter()
                    .any(|reason| reason != Abstain::ValueReference && reasons.contains(reason))
                {
                    continue;
                }
                // Overloads and a static method of the same name give one
                // member each; the companion reads a method name once.
                let mut methods: Vec<String> = Vec::new();
                for member in &class.export.members {
                    if member.kind == MemberKind::ClassMethod
                        && !member.has_decorator
                        && !methods.contains(&member.name)
                    {
                        methods.push(member.name.clone());
                    }
                }
                if methods.is_empty() {
                    continue;
                }
                out.push(MethodClassCandidate {
                    path: self.path_of(module.file_id),
                    class_name: class.local.to_string(),
                    methods,
                    suppressed_lines: self.suppressed_lines(module.file_id),
                });
            }
        }
        out
    }

    /// The lines of the file of a class where a `constant-parameter`
    /// suppression applies. The class span does not hold each line of its
    /// methods, so the scan reads each line of the file.
    fn suppressed_lines(&self, file: FileId) -> Vec<u32> {
        let lines = self
            .input
            .line_offsets_by_file
            .get(&file)
            .map_or(0, |offsets| offsets.len());
        (1..=u32::try_from(lines).unwrap_or(u32::MAX))
            .filter(|line| (self.input.is_suppressed)(file, *line))
            .collect()
    }
}
