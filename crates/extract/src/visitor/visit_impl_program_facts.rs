//! File-level facts that `visit_program` records before the walk.

use oxc_ast::ast::{Class, Program};
use rustc_hash::FxHashMap;

use fallow_types::extract::{DecoratorMetadataReferenceFact, SemanticFact};

use crate::visitor::ModuleInfoExtractor;
use crate::visitor::declarations::statement_is_instantiated;
use crate::visitor::decorator_metadata;

impl ModuleInfoExtractor {
    /// Records whether a top-level statement emits runtime code. Import and
    /// export records already classify their runtime surface, so this fact
    /// covers only the statements that those records do not cover.
    pub(super) fn record_top_level_runtime_statement(&mut self, program: &Program<'_>) {
        if program.body.iter().any(|statement| {
            !statement.is_module_declaration() && statement_is_instantiated(statement)
        }) {
            self.semantic_facts
                .push(SemanticFact::TopLevelRuntimeStatement);
        }
    }

    /// Records the names that the decorator metadata of `class` emits, each
    /// once. A name that one position emits in each mode replaces the same
    /// name that another position emits only without `strictNullChecks`.
    pub(super) fn record_decorator_metadata(&mut self, class: &Class<'_>) {
        let names = decorator_metadata::class_metadata_names(class);
        if names.is_empty() {
            return;
        }
        let mut positions: FxHashMap<String, usize> = self
            .semantic_facts
            .iter()
            .enumerate()
            .filter_map(|(position, fact)| match fact {
                SemanticFact::DecoratorMetadataReference(fact) => {
                    Some((fact.local.clone(), position))
                }
                _ => None,
            })
            .collect();
        for name in names {
            if let Some(&position) = positions.get(&name.local) {
                if let SemanticFact::DecoratorMetadataReference(existing) =
                    &mut self.semantic_facts[position]
                {
                    existing.without_strict_null_checks &= name.without_strict_null_checks;
                }
                continue;
            }
            positions.insert(name.local.clone(), self.semantic_facts.len());
            self.semantic_facts
                .push(SemanticFact::DecoratorMetadataReference(
                    DecoratorMetadataReferenceFact {
                        local: name.local,
                        without_strict_null_checks: name.without_strict_null_checks,
                    },
                ));
        }
    }

    /// Records the file-level string directives (`"use client"`, `"use server"`)
    /// for the security client-server-leak detector. `directive.directive` is
    /// the cooked directive text without surrounding quotes.
    pub(super) fn record_program_directives(&mut self, program: &Program<'_>) {
        for directive in &program.directives {
            self.directives
                .push(directive.directive.as_str().to_string());
        }
    }
}
