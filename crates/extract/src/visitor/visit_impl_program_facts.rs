//! File-level facts that `visit_program` records before the walk.

use oxc_ast::ast::Program;

use fallow_types::extract::SemanticFact;

use crate::visitor::ModuleInfoExtractor;
use crate::visitor::declarations::statement_is_instantiated;

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
