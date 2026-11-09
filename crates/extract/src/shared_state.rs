//! Direct property accesses with lexical binding provenance.

use fallow_types::extract::ImportInfo;
use fallow_types::shared_state::{StateAccessFact, StateOperation, StateRoot};
use oxc_ast::{AstKind, ast::Expression};
use oxc_semantic::{AstNodes, NodeId, Semantic};
use oxc_span::{GetSpan, Span};
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

pub fn collect(
    semantic: &Semantic<'_>,
    imports: &[ImportInfo],
    mode: fallow_types::shared_state::ExtractionMode,
) -> Vec<StateAccessFact> {
    if mode == fallow_types::shared_state::ExtractionMode::Standard {
        return Vec::new();
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
        let Some((path, operation)) = access(semantic.nodes(), node.id(), identifier.span) else {
            continue;
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
    facts
}

fn access(
    nodes: &AstNodes<'_>,
    mut node: NodeId,
    mut span: Span,
) -> Option<(Vec<String>, StateOperation)> {
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
                let key = match &member.expression {
                    Expression::StringLiteral(value) => value.value.as_str()?.to_owned(),
                    Expression::NumericLiteral(value) => value.value.to_string(),
                    _ => return None,
                };
                (key, member.span)
            }
            AstKind::PrivateFieldExpression(_) => return None,
            AstKind::ParenthesizedExpression(wrapper) => {
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
            return None;
        }
        node = parent;
        span = next_span;
    }
    if path.is_empty() {
        return None;
    }
    let operation = match nodes.parent_kind(node) {
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
    };
    Some((path, operation))
}

#[cfg(all(test, not(miri)))]
mod tests {
    use oxc_allocator::Allocator;
    use oxc_ast_visit::Visit;
    use oxc_parser::Parser;
    use oxc_semantic::SemanticBuilder;
    use oxc_span::SourceType;

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
        let paths: Vec<_> = facts.iter().map(|fact| fact.path.clone()).collect();
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
