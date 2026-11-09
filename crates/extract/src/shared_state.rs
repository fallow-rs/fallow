//! Direct property accesses with lexical binding provenance.

use fallow_types::extract::ImportInfo;
use fallow_types::shared_state::{StateAccessFact, StateOperation, StateRoot};
use oxc_ast::{AstKind, ast::Expression};
use oxc_semantic::{AstNodes, NodeId, Semantic};
use oxc_span::{GetSpan, Span};
use oxc_syntax::number::ToJsString;
use rustc_hash::FxHashMap;

/// Maximum retained facts per file. One extra fact signals truncation.
pub const MAX_STATE_FACTS: usize = 4096;
const MAX_PROPERTY_SEGMENTS: usize = 64;

/// Keep one file's merged embedded facts within the extraction bound.
pub fn bound_merged_facts(facts: &mut Vec<fallow_types::extract::SemanticFact>) {
    use fallow_types::extract::SemanticFact;
    if facts.len() <= MAX_STATE_FACTS {
        return;
    }
    let mut retained = 0;
    let mut truncated = false;
    let mut marker = false;
    facts.retain(|fact| {
        if matches!(fact, SemanticFact::SharedStateTruncated) {
            let first = !marker;
            marker = true;
            return first;
        }
        if !matches!(fact, SemanticFact::SharedState(_)) {
            return true;
        }
        retained += 1;
        if retained > MAX_STATE_FACTS {
            truncated = true;
            return false;
        }
        true
    });
    if truncated && !marker {
        facts.push(SemanticFact::SharedStateTruncated);
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct AccessFacts {
    pub facts: Vec<StateAccessFact>,
    pub truncated: bool,
}

enum Access {
    Unsupported,
    Truncated,
    Site(Vec<String>, StateOperation),
}

pub fn collect(
    semantic: &Semantic<'_>,
    imports: &[ImportInfo],
    mode: fallow_types::shared_state::ExtractionMode,
) -> AccessFacts {
    if mode == fallow_types::shared_state::ExtractionMode::Standard {
        return AccessFacts::default();
    }
    let bindings: FxHashMap<_, _> = imports
        .iter()
        .enumerate()
        .filter_map(|(index, import)| {
            crate::binding_references::admitted_import_symbol(semantic, import)
                .map(|symbol| (symbol, index as u32))
        })
        .collect();
    let mut facts = Vec::new();
    let mut truncated = false;
    for node in semantic.nodes().iter() {
        let AstKind::IdentifierReference(identifier) = node.kind() else {
            continue;
        };
        let Some(reference_id) = identifier.reference_id.get() else {
            continue;
        };
        let reference = semantic.scoping().get_reference(reference_id);
        if !reference.is_value() {
            continue;
        }
        let root = match reference.symbol_id() {
            Some(symbol) => {
                let Some(&import_index) = bindings.get(&symbol) else {
                    continue;
                };
                StateRoot::Import { import_index }
            }
            None if matches!(
                identifier.name.as_str(),
                "window" | "globalThis" | "self" | "global"
            ) =>
            {
                StateRoot::Global {
                    name: identifier.name.to_string(),
                }
            }
            None => continue,
        };
        let (path, operation) = match access(semantic.nodes(), node.id(), identifier.span) {
            Access::Unsupported => continue,
            Access::Truncated => {
                truncated = true;
                continue;
            }
            Access::Site(path, operation) => (path, operation),
        };
        if matches!(root, StateRoot::Import { .. }) && operation == StateOperation::Read {
            continue;
        }
        facts.push(StateAccessFact {
            root,
            path,
            operation,
            span_start: identifier.span.start,
        });
        if facts.len() > MAX_STATE_FACTS {
            break;
        }
    }
    facts.sort_by_key(|fact| fact.span_start);
    AccessFacts { facts, truncated }
}

fn access(nodes: &AstNodes<'_>, mut node: NodeId, mut span: Span) -> Access {
    let mut path = Vec::new();
    loop {
        let parent = nodes.parent_id(node);
        if parent == node {
            break;
        }
        let (key, next_span) = match nodes.kind(parent) {
            AstKind::StaticMemberExpression(member) if member.object.span() == span => {
                (member.property.name.to_string(), member.span)
            }
            AstKind::ComputedMemberExpression(member) if member.object.span() == span => {
                let Some(key) = static_key(&member.expression) else {
                    return Access::Unsupported;
                };
                (key, member.span)
            }
            AstKind::PrivateFieldExpression(_) => return Access::Unsupported,
            AstKind::ParenthesizedExpression(wrapper) => {
                node = parent;
                span = wrapper.span;
                continue;
            }
            AstKind::TSTypeAssertion(wrapper) => {
                node = parent;
                span = wrapper.span;
                continue;
            }
            AstKind::TSAsExpression(wrapper) => {
                node = parent;
                span = wrapper.span;
                continue;
            }
            AstKind::TSNonNullExpression(wrapper) => {
                node = parent;
                span = wrapper.span;
                continue;
            }
            AstKind::TSSatisfiesExpression(wrapper) => {
                node = parent;
                span = wrapper.span;
                continue;
            }
            _ => break,
        };
        path.push(key);
        if path.len() > MAX_PROPERTY_SEGMENTS {
            return Access::Truncated;
        }
        node = parent;
        span = next_span;
    }
    if path.is_empty() {
        return Access::Unsupported;
    }
    let parent = nodes.parent_kind(node);
    if unsupported_target(parent, span) {
        return Access::Unsupported;
    }
    Access::Site(path, operation(parent, span))
}

fn static_key(expression: &Expression<'_>) -> Option<String> {
    match expression {
        Expression::StringLiteral(value) => value.value.as_str().map(str::to_owned),
        Expression::NumericLiteral(value) => Some(value.value.to_js_string()),
        _ => None,
    }
}

fn unsupported_target(kind: AstKind<'_>, span: Span) -> bool {
    match kind {
        AstKind::ArrayAssignmentTarget(_) | AstKind::AssignmentTargetRest(_) => true,
        AstKind::AssignmentTargetPropertyProperty(property) => property.binding.span() == span,
        AstKind::AssignmentTargetWithDefault(target) => target.binding.span() == span,
        AstKind::ForInStatement(statement) => statement.left.span() == span,
        AstKind::ForOfStatement(statement) => statement.left.span() == span,
        _ => false,
    }
}

fn operation(kind: AstKind<'_>, span: Span) -> StateOperation {
    match kind {
        AstKind::AssignmentExpression(assignment) if assignment.left.span() == span => {
            if assignment.operator.is_assign() {
                StateOperation::Assign
            } else {
                StateOperation::ReadModifyWrite
            }
        }
        AstKind::UpdateExpression(update) if update.argument.span() == span => {
            StateOperation::Update
        }
        AstKind::UnaryExpression(unary)
            if unary.operator.as_str() == "delete" && unary.argument.span() == span =>
        {
            StateOperation::Delete
        }
        _ => StateOperation::Read,
    }
}

#[cfg(all(test, not(miri)))]
mod tests {
    use oxc_allocator::Allocator;
    use oxc_ast_visit::Visit;
    use oxc_parser::Parser;
    use oxc_semantic::SemanticBuilder;
    use oxc_span::SourceType;

    fn extracted(source: &str) -> Vec<fallow_types::shared_state::StateAccessFact> {
        let module = crate::parse_source_to_module_with_mode(
            crate::FileId(0),
            std::path::Path::new("entry.ts"),
            source,
            0,
            false,
            fallow_types::shared_state::ExtractionMode::SharedState,
        );
        assert_eq!(module.parse_error_count, 0);
        module
            .semantic_facts
            .iter()
            .filter_map(|fact| match fact {
                fallow_types::extract::SemanticFact::SharedState(fact) => Some(fact.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn shared_state_unsupported_targets_preserve_real_reads() {
        use fallow_types::shared_state::StateOperation;
        for source in [
            "({value: window.custom} = {value: window.rhs});",
            "[window.custom] = [window.rhs];",
            "[...window.custom] = window.rhs;",
            "({value: window.custom = window.rhs} = {});",
            "for (window.custom of window.rhs) {}",
            "for (window.custom in window.rhs) {}",
            "({[window.rhs]: window.custom} = {});",
        ] {
            let facts = extracted(source);
            assert_eq!(
                facts
                    .iter()
                    .map(|fact| (&fact.path, fact.operation))
                    .collect::<Vec<_>>(),
                vec![(&vec!["rhs".to_owned()], StateOperation::Read)],
                "{source}"
            );
        }
        let facts = extracted("target[window.key] = window.rhs;");
        assert_eq!(
            facts
                .iter()
                .map(|fact| fact.path[0].as_str())
                .collect::<Vec<_>>(),
            vec!["key", "rhs"]
        );
    }

    #[test]
    fn shared_state_numeric_keys_use_ecmascript_spelling() {
        let facts = extracted(
            r#"window[1e21] = 1; window["1e+21"]; window["1000000000000000000000"];
window[1e-7] = 1; window["1e-7"]; window["0.0000001"]; window[42] = 1; window["42"];"#,
        );
        assert_eq!(
            facts
                .iter()
                .map(|fact| fact.path[0].as_str())
                .collect::<Vec<_>>(),
            vec![
                "1e+21",
                "1e+21",
                "1000000000000000000000",
                "1e-7",
                "1e-7",
                "0.0000001",
                "42",
                "42"
            ]
        );
    }

    #[test]
    fn shared_state_type_assertions_preserve_import_and_global_writes() {
        use fallow_types::shared_state::{StateOperation, StateRoot};
        let facts = extracted(
            "import {state} from './owner'; (<any>state).custom = 1; (state as any).other = 1; (<any>window).custom = 1; (window as any).other = 1;",
        );
        assert_eq!(
            facts
                .iter()
                .map(|fact| fact.path[0].as_str())
                .collect::<Vec<_>>(),
            vec!["custom", "other", "custom", "other"]
        );
        assert!(
            facts
                .iter()
                .all(|fact| fact.operation == StateOperation::Assign)
        );
        assert!(matches!(
            facts[0].root,
            StateRoot::Import { import_index: 0 }
        ));
        assert_eq!(
            facts[2].root,
            StateRoot::Global {
                name: "window".to_owned()
            }
        );
    }

    #[test]
    fn imported_writes_and_unshadowed_globals_preserve_paths() {
        let source = r#"import { state as config } from './owner';
config.x = 1; config["a.b"]++; config.a.b ||= 2; delete config.old;
function f(config, window) { config.x = 2; window.custom = 3; }
window.custom = 4; globalThis.custom; typeof window.custom;
type T = typeof window.custom; config[key] = 5; config.push(1);
"#;
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
        let semantic = SemanticBuilder::new()
            .with_build_nodes(true)
            .build(&parsed.program)
            .semantic;
        let mut extractor = crate::visitor::ModuleInfoExtractor::new();
        extractor.visit_program(&parsed.program);
        let facts = super::collect(
            &semantic,
            &extractor.imports,
            fallow_types::shared_state::ExtractionMode::SharedState,
        );
        let paths: Vec<_> = facts.facts.iter().map(|fact| fact.path.clone()).collect();
        assert_eq!(
            paths,
            vec![
                vec!["x"],
                vec!["a.b"],
                vec!["a", "b"],
                vec!["old"],
                vec!["custom"],
                vec!["custom"],
                vec!["custom"]
            ]
        );
    }
    #[test]
    fn shared_state_embedded_sources_keep_offsets_and_import_indices() {
        use fallow_types::extract::SemanticFact;
        use fallow_types::shared_state::{ExtractionMode, StateRoot};
        for (filename, source) in [
            (
                "view.vue",
                "<script>import {state} from './one'; state.x = 1;</script>\n<script setup>import {state} from './two'; state.y = 2;</script>",
            ),
            (
                "view.astro",
                "---\nimport {state} from './one'; state.x = 1;\n---\n<script>import {state} from './two'; state.y = 2;</script>",
            ),
        ] {
            let parse = |mode| {
                crate::parse_source_to_module_with_mode(
                    crate::FileId(0),
                    std::path::Path::new(filename),
                    source,
                    0,
                    false,
                    mode,
                )
            };
            let normal = parse(ExtractionMode::Standard);
            assert!(
                !normal
                    .semantic_facts
                    .iter()
                    .any(|fact| matches!(fact, SemanticFact::SharedState(_)))
            );
            let module = parse(ExtractionMode::SharedState);
            let facts: Vec<_> = module
                .semantic_facts
                .iter()
                .filter_map(|fact| {
                    if let SemanticFact::SharedState(fact) = fact {
                        Some(fact)
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(facts.len(), 2, "{filename}");
            for (fact, expected_source, expected_text) in [
                (facts[0], "./one", "state.x"),
                (facts[1], "./two", "state.y"),
            ] {
                let StateRoot::Import { import_index } = fact.root else {
                    panic!("import provenance")
                };
                assert_eq!(
                    module.imports[import_index as usize].source,
                    expected_source
                );
                assert!(source[fact.span_start as usize..].starts_with(expected_text));
            }
        }
    }
}

#[cfg(all(test, not(miri)))]
mod bounded_tests {
    use fallow_types::extract::SemanticFact;
    use fallow_types::shared_state::ExtractionMode;

    #[test]
    fn shared_state_retention_is_explicit_and_absent_in_standard_mode() {
        let source = "window.custom = 1;\n".repeat(super::MAX_STATE_FACTS + 1);
        let parse = |mode| {
            crate::parse_source_to_module_with_mode(
                crate::FileId(0),
                std::path::Path::new("entry.ts"),
                &source,
                0,
                false,
                mode,
            )
        };
        assert!(
            !parse(ExtractionMode::Standard)
                .semantic_facts
                .iter()
                .any(|fact| matches!(
                    fact,
                    SemanticFact::SharedState(_) | SemanticFact::SharedStateTruncated
                ))
        );
        let inventory = parse(ExtractionMode::SharedState);
        assert!(
            inventory
                .semantic_facts
                .contains(&SemanticFact::SharedStateTruncated)
        );
        assert_eq!(
            inventory
                .semantic_facts
                .iter()
                .filter(|fact| matches!(fact, SemanticFact::SharedState(_)))
                .count(),
            super::MAX_STATE_FACTS
        );
    }
}

#[cfg(all(test, not(miri)))]
mod embedded_limit_tests {
    #[test]
    fn shared_state_embedded_retention_limits_the_whole_file() {
        use fallow_types::extract::SemanticFact;
        let script = format!(
            "<script>{}</script>",
            "window.custom = 1;".repeat(super::MAX_STATE_FACTS / 2 + 1)
        );
        for file in ["entry.vue", "entry.astro"] {
            let module = crate::parse_source_to_module_with_mode(
                crate::FileId(0),
                std::path::Path::new(file),
                &script.repeat(2),
                0,
                false,
                fallow_types::shared_state::ExtractionMode::SharedState,
            );
            assert_eq!(
                module
                    .semantic_facts
                    .iter()
                    .filter(|fact| matches!(fact, SemanticFact::SharedState(_)))
                    .count(),
                super::MAX_STATE_FACTS,
                "{file}"
            );
            assert!(
                module
                    .semantic_facts
                    .contains(&SemanticFact::SharedStateTruncated)
            );
        }
    }
}
