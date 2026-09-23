//! CSS-in-JS design-token DEFINITION walker for the design-token blast-radius
//! (CSS program Phase 3d).
//!
//! The zero-runtime CSS-in-JS libraries declare design tokens as a JS OBJECT
//! passed to a library call, binding the token surface to an exported identifier
//! that consumers read via member access (`import { vars } from './tokens';
//! vars.color.primary`). This module is the DEFINITION half of the token
//! blast-radius: it parses JS/TS with oxc, gates recognition on import-binding
//! provenance (reusing the sibling `object::module_library`), and for each
//! recognized token-definition call emits the access BINDING plus the flattened
//! dotted LEAF token paths (with each leaf's source line). The CONSUMER half (who
//! reads `vars.color.primary` across modules) is resolved in the analyze layer
//! against the module graph; this walker only produces the defined-token side.
//!
//! Health-time-only, like the 3b/3c CSS-in-JS lifters: it runs over file SOURCE
//! and persists nothing to the extraction cache (no `CACHE_VERSION` bump).
//!
//! # Recognized definition shapes
//!
//! Recognition is gated on the callee binding being imported from a recognized
//! token library in THIS file (a local `defineVars` helper or an unrelated
//! `createTheme` never fires):
//!
//! - StyleX `stylex.defineVars({...})` (namespace member call) or
//!   `defineVars({...})` (named import). Binding = the assigned identifier and
//!   every top-level key is one token, including conditional object values.
//! - StyleX `unstable_defineVarsNested({...})`: binding = the assigned identifier;
//!   namespace objects recurse while conditional objects and calls remain leaves.
//! - vanilla-extract `createThemeContract({...})`: binding = the assigned
//!   identifier (the contract IS the vars surface consumers read).
//! - vanilla-extract `createTheme({...})` (1-arg): returns `[themeClass, vars]`;
//!   binding = the SECOND array-destructure element (`vars`); `themeClass` is a
//!   class string, not a token surface.
//! - vanilla-extract `createGlobalTheme(selector, {...})` (2-arg): returns the
//!   vars object; binding = the assigned identifier.
//! - PandaCSS `defineTokens({...})`: binding = the assigned identifier; token
//!   objects with a `value` field collapse to the token path (`colors.brand`),
//!   matching `token('colors.brand')` consumers.
//! - PandaCSS `defineConfig({ theme: { tokens, semanticTokens } })`: binding =
//!   `pandaConfig`; only static token object literals are read.
//!
//! The two CONTRACT-IMPLEMENTATION forms are deliberately NOT definition sites
//! here, because the contract they fill was already declared by
//! `createThemeContract` (captured above) and that is the binding consumers read:
//! - `createTheme(contract, {...})` (2-arg) returns a class string; tokens fill
//!   the existing `contract`.
//! - `createGlobalTheme(selector, contract, {...})` (3-arg) returns void.
//!
use std::path::Path;

use oxc_allocator::Allocator;
use oxc_ast::{
    AstKind,
    ast::{
        Argument, ArrowFunctionExpression, AssignmentExpression, AssignmentTarget,
        AssignmentTargetMaybeDefault, AssignmentTargetProperty, BindingPattern, BlockStatement,
        CallExpression, ComputedMemberExpression, Declaration, Expression, Function,
        IdentifierReference, ImportDeclarationSpecifier, NumericLiteral, ObjectExpression,
        ObjectPropertyKind, Program, SimpleAssignmentTarget, Statement, StaticMemberExpression,
        UnaryExpression, UnaryOperator, UpdateExpression, VariableDeclarationKind,
        VariableDeclarator,
    },
};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_semantic::{ReferenceId, ScopeFlags, Scoping, SemanticBuilder, SymbolId};
use oxc_span::{GetSpan, SourceType, Span};
use rustc_hash::{FxHashMap, FxHashSet};

use super::object::{Lib, module_library};

const PANDA_CONFIG_BINDING: &str = "pandaConfig";

/// A single defined design token: its dotted LEAF path relative to the access
/// binding (`color.primary`, or flat `primaryColor` for StyleX), the 1-based
/// source line of its key, and the static value when the literal is recoverable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssInJsToken {
    /// Dotted leaf path relative to the binding (e.g. `color.primary`).
    pub path: String,
    /// 1-based line of the token's key in the defining source.
    pub def_line: u32,
    /// Static token value for literal definitions. Dynamic expressions and
    /// contract-only leaves have no value.
    pub value: Option<String>,
}

/// A CSS-in-JS token-definition site: the exported access binding consumers read
/// through (e.g. `vars`) and the flattened leaf tokens it defines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssInJsTokenDef {
    /// The identifier the token surface is bound to (`vars`), the receiver of
    /// cross-module member access (`vars.color.primary`).
    pub binding: String,
    /// Which CSS-in-JS family defined the tokens.
    pub origin: CssInJsTokenOrigin,
    /// The flattened leaf tokens defined on `binding`.
    pub tokens: Vec<CssInJsToken>,
}

/// The CSS-in-JS token system that produced a token definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CssInJsTokenOrigin {
    /// StyleX `defineVars`.
    StyleX,
    /// vanilla-extract `createTheme` family definitions.
    VanillaExtract,
    /// PandaCSS `defineTokens`.
    Panda,
    /// styled-components / Emotion theme object definitions.
    Theme,
}

/// Walk a JS/TS source for CSS-in-JS design-token DEFINITIONS, returning each
/// access binding and its flattened leaf token paths. Empty when the source has
/// no recognized token-library import (provenance gate closed).
#[must_use]
pub fn css_in_js_token_defs(source: &str, path: &Path) -> Vec<CssInJsTokenDef> {
    let source_type = SourceType::from_path(path).unwrap_or_default();
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, source_type).parse();

    let mut collector = TokenDefCollector::new(source);
    collector.build_import_map(&ret.program);
    if collector.imports.is_empty() {
        return Vec::new();
    }
    let semantic_return = SemanticBuilder::new().build(&ret.program);
    collector.build_const_object_map(&ret.program, semantic_return.semantic.scoping());
    collector.collecting_mutations = true;
    collector.visit_program(&ret.program);
    collector.collecting_mutations = false;
    collector.visit_program(&ret.program);
    collector.defs
}

/// One located consumer of a CSS-in-JS token: the defined LEAF token path it
/// reads (relative to the binding, e.g. `color.primary`) and the 1-based line of
/// the member-access site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenConsumerHit {
    /// The defined leaf token path consumed (`color.primary`), relative to the
    /// access binding (the leading binding segment stripped).
    pub token_path: String,
    /// 1-based line of the member-access site in the consuming source.
    pub line: u32,
}

/// Walk a JS/TS source for statically-authored theme object definitions used by
/// styled-components and Emotion. A `theme` or `*Theme` variable with an object
/// literal initializer becomes a token surface, with nested scalar leaves exposed
/// as dotted paths.
#[must_use]
pub fn css_in_js_theme_token_defs(source: &str, path: &Path) -> Vec<CssInJsTokenDef> {
    let source_type = SourceType::from_path(path).unwrap_or_default();
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, source_type).parse();

    let mut collector = ThemeDefCollector {
        lines: LineCounter::new(source),
        defs: Vec::new(),
    };
    collector.visit_program(&ret.program);
    collector.defs
}

/// One attribution query to run against a single parsed consumer source. A scan
/// runs any mix of queries against ONE parse of the source.
pub enum ConsumerQuery<'a> {
    /// Member-access reads `<alias>.a.b` of an imported token binding. A read is
    /// a hit only when `a.b` is a defined leaf path, not an intermediate group.
    MemberBinding {
        /// The local identifier the token binding was imported under.
        alias: &'a str,
        /// The defined leaf token paths (`color.primary`).
        leaf_paths: &'a FxHashSet<String>,
    },
    /// StyleX `createTheme(contract, ...)` applies the complete resolved
    /// variable group, including partial and empty reset themes.
    StyleXThemeGroup {
        /// Local identifier of the resolved StyleX variable contract.
        contract_alias: &'a str,
        /// Every defined leaf in that contract group.
        leaf_paths: &'a FxHashSet<String>,
    },
    /// PandaCSS `token('a.b')` calls through the given alias.
    PandaTokenCall {
        /// The local alias imported from Panda's generated token module.
        alias: &'a str,
        /// The defined leaf token paths (`colors.brand`).
        leaf_paths: &'a FxHashSet<String>,
    },
    /// PandaCSS style-call object values naming token paths.
    PandaStyleValues {
        /// The local aliases for Panda style calls (`css`, `cva`).
        aliases: &'a FxHashSet<String>,
        /// The defined leaf token paths (`colors.brand`).
        leaf_paths: &'a FxHashSet<String>,
    },
    /// styled-components / Emotion theme reads (`theme.colors.x`), including
    /// `props.theme.colors.x`.
    ThemeReads {
        /// The defined leaf token paths (`colors.brand`).
        leaf_paths: &'a FxHashSet<String>,
    },
}

impl ConsumerQuery<'_> {
    /// `true` for the queries that `BatchedBindingCollector` runs in one walk.
    const fn is_batched(&self) -> bool {
        matches!(
            self,
            Self::MemberBinding { .. } | Self::StyleXThemeGroup { .. }
        )
    }
}

/// Parse `source` once and run every query against the same AST, returning
/// `(query_index, hit)` pairs so the caller can attribute each hit back to the
/// definer that produced its query. A query with an empty alias or an empty
/// leaf set contributes no hits and does not suppress the other queries.
#[must_use]
pub fn css_in_js_consumer_scan(
    source: &str,
    path: &Path,
    queries: &[ConsumerQuery<'_>],
) -> Vec<(usize, TokenConsumerHit)> {
    if queries.is_empty() {
        return Vec::new();
    }
    let source_type = SourceType::from_path(path).unwrap_or_default();
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, source_type).parse();
    let mut out = Vec::new();
    if queries.iter().any(ConsumerQuery::is_batched) {
        let mut collector = BatchedBindingCollector {
            lines: LineCounter::new(source),
            queries,
            namespaces: FxHashSet::default(),
            theme_functions: FxHashSet::default(),
            stylex_imports: FxHashMap::default(),
            member_queries: FxHashMap::default(),
            theme_queries: FxHashMap::default(),
            root_reference_spans: FxHashSet::default(),
            static_values: FxHashMap::default(),
            static_alias_neighbors: FxHashMap::default(),
            collecting_mutations: false,
            hits: Vec::new(),
        };
        collector.build_import_map(&ret.program);
        collector.build_query_indexes();
        collector.build_static_value_map(&ret.program);
        collector.build_root_reference_spans(&ret.program);
        collector.collecting_mutations = true;
        collector.visit_program(&ret.program);
        collector.collecting_mutations = false;
        collector.visit_program(&ret.program);
        out.extend(collector.hits);
    }
    for (idx, query) in queries.iter().enumerate() {
        if !query.is_batched() {
            run_consumer_query(query, source, &ret.program, idx, &mut out);
        }
    }
    out
}

struct BatchedBindingCollector<'a, 'q, 'v> {
    lines: LineCounter<'a>,
    queries: &'q [ConsumerQuery<'v>],
    namespaces: FxHashSet<&'a str>,
    theme_functions: FxHashSet<&'a str>,
    stylex_imports: FxHashMap<&'a str, (Lib, &'a str)>,
    member_queries: FxHashMap<&'v str, Vec<(usize, &'v FxHashSet<String>)>>,
    theme_queries: FxHashMap<&'v str, Vec<(usize, &'v FxHashSet<String>)>>,
    root_reference_spans: FxHashSet<Span>,
    static_values: FxHashMap<&'a str, (u32, &'a Expression<'a>)>,
    static_alias_neighbors: FxHashMap<&'a str, FxHashSet<&'a str>>,
    collecting_mutations: bool,
    hits: Vec<(usize, TokenConsumerHit)>,
}

impl<'a> BatchedBindingCollector<'a, '_, '_> {
    fn build_import_map(&mut self, program: &'a Program<'a>) {
        for stmt in &program.body {
            let Statement::ImportDeclaration(decl) = stmt else {
                continue;
            };
            if decl.import_kind.is_type()
                || module_library(decl.source.value.as_str()) != Some(Lib::StyleX)
            {
                continue;
            }
            let Some(specifiers) = &decl.specifiers else {
                continue;
            };
            for specifier in specifiers {
                match specifier {
                    ImportDeclarationSpecifier::ImportSpecifier(specifier)
                        if !specifier.import_kind.is_type() =>
                    {
                        let local = specifier.local.name.as_str();
                        let role = specifier.imported.name().as_str();
                        self.stylex_imports.insert(local, (Lib::StyleX, role));
                        if matches!(role, "createTheme" | "unstable_createThemeNested") {
                            self.theme_functions.insert(local);
                        }
                    }
                    ImportDeclarationSpecifier::ImportDefaultSpecifier(specifier) => {
                        let local = specifier.local.name.as_str();
                        self.namespaces.insert(local);
                        self.stylex_imports.insert(local, (Lib::StyleX, local));
                    }
                    ImportDeclarationSpecifier::ImportNamespaceSpecifier(specifier) => {
                        let local = specifier.local.name.as_str();
                        self.namespaces.insert(local);
                        self.stylex_imports.insert(local, (Lib::StyleX, local));
                    }
                    ImportDeclarationSpecifier::ImportSpecifier(_) => {}
                }
            }
        }
    }

    fn build_query_indexes(&mut self) {
        for (idx, query) in self.queries.iter().enumerate() {
            match query {
                ConsumerQuery::MemberBinding { alias, leaf_paths } if !alias.is_empty() => {
                    self.member_queries
                        .entry(alias)
                        .or_default()
                        .push((idx, leaf_paths));
                }
                ConsumerQuery::StyleXThemeGroup {
                    contract_alias,
                    leaf_paths,
                } if !contract_alias.is_empty() => {
                    self.theme_queries
                        .entry(contract_alias)
                        .or_default()
                        .push((idx, leaf_paths));
                }
                _ => {}
            }
        }
    }

    fn build_root_reference_spans(&mut self, program: &'a Program<'a>) {
        let mut names: FxHashSet<&str> = self.member_queries.keys().copied().collect();
        names.extend(self.theme_queries.keys().copied());
        names.extend(self.namespaces.iter().copied());
        names.extend(self.theme_functions.iter().copied());
        names.extend(self.stylex_imports.keys().copied());
        names.extend(self.static_values.keys().copied());

        let semantic_return = SemanticBuilder::new().build(program);
        let semantic = semantic_return.semantic;
        let scoping = semantic.scoping();
        let root_scope = scoping.root_scope_id();
        for name in names {
            if let Some(symbol_id) = scoping.get_binding(root_scope, oxc_str::Ident::from(name)) {
                for reference in scoping.get_resolved_references(symbol_id) {
                    if let AstKind::IdentifierReference(identifier) =
                        semantic.nodes().kind(reference.node_id())
                    {
                        self.root_reference_spans.insert(identifier.span);
                    }
                }
            }
            if let Some(reference_ids) = scoping.root_unresolved_references().get(name) {
                for reference_id in reference_ids {
                    let reference = scoping.get_reference(*reference_id);
                    if let AstKind::IdentifierReference(identifier) =
                        semantic.nodes().kind(reference.node_id())
                    {
                        self.root_reference_spans.insert(identifier.span);
                    }
                }
            }
        }
        for name in ["String", "Number", "Math", "Object", "Array"] {
            if let Some(reference_ids) = scoping.root_unresolved_references().get(name) {
                for reference_id in reference_ids {
                    let reference = scoping.get_reference(*reference_id);
                    if let AstKind::IdentifierReference(identifier) =
                        semantic.nodes().kind(reference.node_id())
                    {
                        self.root_reference_spans.insert(identifier.span);
                    }
                }
            }
        }
    }

    fn build_static_value_map(&mut self, program: &'a Program<'a>) {
        for stmt in &program.body {
            let declaration = match stmt {
                Statement::VariableDeclaration(declaration) => Some(&**declaration),
                Statement::ExportNamedDeclaration(export) => match &export.declaration {
                    Some(Declaration::VariableDeclaration(declaration)) => Some(&**declaration),
                    _ => None,
                },
                _ => None,
            };
            let Some(declaration) = declaration else {
                continue;
            };
            if declaration.kind != VariableDeclarationKind::Const {
                continue;
            }
            for declarator in &declaration.declarations {
                let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
                    continue;
                };
                if let Some(init) = &declarator.init {
                    self.static_values
                        .insert(binding.name.as_str(), (declarator.span.start, init));
                    if let Some(alias) = expression_root_binding(init) {
                        self.static_alias_neighbors
                            .entry(binding.name.as_str())
                            .or_default()
                            .insert(alias.name);
                        self.static_alias_neighbors
                            .entry(alias.name)
                            .or_default()
                            .insert(binding.name.as_str());
                    }
                }
            }
        }
    }

    fn is_theme_callee(&self, callee: &Expression<'a>) -> bool {
        match callee {
            Expression::Identifier(id) => {
                self.root_reference_spans.contains(&id.span)
                    && self.theme_functions.contains(id.name.as_str())
            }
            Expression::StaticMemberExpression(member) => {
                let Expression::Identifier(object) = unwrap_transparent_expression(&member.object)
                else {
                    return false;
                };
                self.root_reference_spans.contains(&object.span)
                    && self.namespaces.contains(object.name.as_str())
                    && matches!(
                        member.property.name.as_str(),
                        "createTheme" | "unstable_createThemeNested"
                    )
            }
            _ => false,
        }
    }

    fn record_member(&mut self, chain: Option<(&str, Span, Vec<String>)>, span_start: u32) {
        let Some((base, base_span, segments)) = chain else {
            return;
        };
        if segments.is_empty() || !self.root_reference_spans.contains(&base_span) {
            return;
        }
        let token_path = segments.join(".");
        if let Some(queries) = self.member_queries.get(base) {
            for &(idx, leaf_paths) in queries {
                if !leaf_paths.contains(&token_path) {
                    continue;
                }
                let line = self.lines.line_at(span_start);
                self.hits.push((
                    idx,
                    TokenConsumerHit {
                        token_path: token_path.clone(),
                        line,
                    },
                ));
            }
        }
    }

    fn invalidate_static_value(&mut self, binding: Option<RootBinding<'_>>) {
        if let Some(binding) = binding
            && self.root_reference_spans.contains(&binding.span)
        {
            let mut pending = vec![binding.name];
            let mut invalidated = FxHashSet::default();
            while let Some(name) = pending.pop() {
                if !invalidated.insert(name) {
                    continue;
                }
                self.static_values.remove(name);
                if let Some(neighbors) = self.static_alias_neighbors.get(name) {
                    pending.extend(neighbors.iter().copied());
                }
            }
        }
    }

    fn mutation_root_binding(
        &self,
        expression: &'a Expression<'a>,
        before: u32,
        visiting: &mut FxHashSet<&'a str>,
    ) -> Option<RootBinding<'a>> {
        if let Some(binding) = expression_root_binding(expression) {
            return Some(binding);
        }
        let Expression::CallExpression(call) = unwrap_transparent_expression(expression) else {
            return None;
        };
        let Expression::Identifier(callee) = unwrap_transparent_expression(&call.callee) else {
            return None;
        };
        if !self.root_reference_spans.contains(&callee.span) {
            return None;
        }
        let name = callee.name.as_str();
        let &(declaration_start, value) = self.static_values.get(name)?;
        let Expression::ArrowFunctionExpression(arrow) = unwrap_transparent_expression(value)
        else {
            return None;
        };
        if declaration_start >= before
            || arrow.r#async
            || arrow.params.rest.is_some()
            || arrow.params.items.len() != call.arguments.len()
            || !visiting.insert(name)
        {
            return None;
        }
        let Some(body) = stylex_arrow_expression_body(arrow) else {
            visiting.remove(name);
            return None;
        };
        let resolved = expression_root_binding(body)
            .and_then(|body_root| {
                arrow.params.items.iter().position(|parameter| {
                    matches!(
                        &parameter.pattern,
                        BindingPattern::BindingIdentifier(binding)
                            if binding.name.as_str() == body_root.name
                    )
                })
            })
            .and_then(|index| call.arguments.get(index))
            .and_then(Argument::as_expression)
            .and_then(|argument| self.mutation_root_binding(argument, before, visiting))
            .or_else(|| self.mutation_root_binding(body, declaration_start, visiting));
        visiting.remove(name);
        resolved
    }

    fn binding_is_definitely_primitive(&self, binding: RootBinding<'_>, before: u32) -> bool {
        let Some(&(declaration_start, value)) = self.static_values.get(binding.name) else {
            return false;
        };
        declaration_start < before
            && is_definitely_static_primitive(
                value,
                declaration_start,
                &self.static_values,
                &self.root_reference_spans,
                &mut FxHashSet::default(),
            )
    }

    fn record_theme_call(&mut self, call: &CallExpression<'a>) {
        if call.arguments.len() != 2 || !self.is_theme_callee(&call.callee) {
            return;
        }
        let Some(contract_expression) = call.arguments.first().and_then(Argument::as_expression)
        else {
            return;
        };
        let Some(contract) = self.resolve_theme_contract_alias(
            contract_expression,
            call.span.start,
            &mut FxHashSet::default(),
        ) else {
            return;
        };
        let Some(overrides) = call.arguments.get(1).and_then(Argument::as_expression) else {
            return;
        };
        if !is_static_stylex_theme_override_object(
            overrides,
            call.span.start,
            &self.static_values,
            &self.stylex_imports,
            &self.root_reference_spans,
            &mut FxHashSet::default(),
        ) {
            return;
        }
        let Some(queries) = self.theme_queries.get(contract) else {
            return;
        };
        let line = self.lines.line_at(call.span().start);
        for &(idx, leaf_paths) in queries {
            self.hits.extend(
                leaf_paths
                    .iter()
                    .cloned()
                    .map(|token_path| (idx, TokenConsumerHit { token_path, line })),
            );
        }
    }

    fn resolve_theme_contract_alias(
        &self,
        expression: &'a Expression<'a>,
        before: u32,
        visiting: &mut FxHashSet<&'a str>,
    ) -> Option<&'a str> {
        let Expression::Identifier(identifier) = unwrap_transparent_expression(expression) else {
            return None;
        };
        if !self.root_reference_spans.contains(&identifier.span) {
            return None;
        }
        let name = identifier.name.as_str();
        if self.theme_queries.contains_key(name) {
            return Some(name);
        }
        let &(declaration_start, value) = self.static_values.get(name)?;
        if declaration_start >= before || !visiting.insert(name) {
            return None;
        }
        let resolved = self.resolve_theme_contract_alias(value, declaration_start, visiting);
        visiting.remove(name);
        resolved
    }
}

impl<'a> Visit<'a> for BatchedBindingCollector<'a, '_, '_> {
    fn visit_static_member_expression(&mut self, member: &StaticMemberExpression<'a>) {
        let mut chain = binding_access_object_chain(&member.object);
        if let Some((_, _, segments)) = chain.as_mut() {
            segments.push(member.property.name.to_string());
        }
        if !self.collecting_mutations {
            self.record_member(chain, member.span().start);
        }
        walk::walk_static_member_expression(self, member);
    }

    fn visit_computed_member_expression(&mut self, member: &ComputedMemberExpression<'a>) {
        let mut chain = binding_access_object_chain(&member.object);
        if let (Some((_, _, segments)), Some(key)) =
            (chain.as_mut(), static_computed_key(&member.expression))
        {
            segments.push(key);
        } else {
            chain = None;
        }
        if !self.collecting_mutations {
            self.record_member(chain, member.span().start);
        }
        walk::walk_computed_member_expression(self, member);
    }

    fn visit_variable_declarator(&mut self, declaration: &VariableDeclarator<'a>) {
        if !self.collecting_mutations
            && let (BindingPattern::BindingIdentifier(_), Some(Expression::CallExpression(call))) =
                (&declaration.id, declaration.init.as_ref())
        {
            self.record_theme_call(call);
        }
        walk::walk_variable_declarator(self, declaration);
    }

    fn visit_assignment_expression(&mut self, assignment: &AssignmentExpression<'a>) {
        if self.collecting_mutations {
            for binding in assignment_target_root_bindings(&assignment.left) {
                self.invalidate_static_value(Some(binding));
            }
            if let Some(receiver) = assignment_target_receiver_expression(&assignment.left) {
                let binding = self.mutation_root_binding(
                    receiver,
                    assignment.span.start,
                    &mut FxHashSet::default(),
                );
                self.invalidate_static_value(binding);
            }
        }
        walk::walk_assignment_expression(self, assignment);
    }

    fn visit_update_expression(&mut self, update: &UpdateExpression<'a>) {
        if self.collecting_mutations {
            self.invalidate_static_value(simple_assignment_target_root_binding(&update.argument));
        }
        walk::walk_update_expression(self, update);
    }

    fn visit_unary_expression(&mut self, expression: &UnaryExpression<'a>) {
        if self.collecting_mutations && expression.operator.is_delete() {
            self.invalidate_static_value(expression_root_binding(&expression.argument));
        }
        walk::walk_unary_expression(self, expression);
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if !self.collecting_mutations {
            walk::walk_call_expression(self, call);
            return;
        }
        let mut visiting = FxHashSet::default();
        let is_stylex_helper = call.arguments.len() == 1
            && is_root_stylex_static_call(
                &call.callee,
                &self.stylex_imports,
                &self.root_reference_spans,
            )
            && call
                .arguments
                .first()
                .and_then(Argument::as_expression)
                .is_some_and(|argument| {
                    is_static_stylex_theme_override(
                        argument,
                        call.span.start,
                        &self.static_values,
                        &self.stylex_imports,
                        &self.root_reference_spans,
                        &mut visiting,
                    )
                });
        let is_pure = is_stylex_helper
            || is_static_stylex_pure_call(
                call,
                call.span.start,
                &self.static_values,
                &self.stylex_imports,
                &self.root_reference_spans,
                &mut visiting,
            );
        if !self.is_theme_callee(&call.callee) && !is_pure {
            if let Some(receiver) = call_receiver_expression(&call.callee) {
                let binding = self.mutation_root_binding(
                    receiver,
                    call.span.start,
                    &mut FxHashSet::default(),
                );
                self.invalidate_static_value(binding);
            }
            let possibly_mutated: Vec<RootBinding<'_>> = call
                .arguments
                .iter()
                .filter_map(Argument::as_expression)
                .filter_map(|argument| {
                    self.mutation_root_binding(argument, call.span.start, &mut FxHashSet::default())
                })
                .collect();
            for binding in possibly_mutated {
                if !self.binding_is_definitely_primitive(binding, call.span.start) {
                    self.invalidate_static_value(Some(binding));
                }
            }
        }
        walk::walk_call_expression(self, call);
    }
}

/// Run one non-batched [`ConsumerQuery`] against an already-parsed `program`,
/// and tag each resulting hit with `idx`. A query with an empty alias or an
/// empty leaf set gives no hits.
fn run_consumer_query<'a>(
    query: &ConsumerQuery<'_>,
    source: &'a str,
    program: &Program<'a>,
    idx: usize,
    out: &mut Vec<(usize, TokenConsumerHit)>,
) {
    match query {
        // `css_in_js_consumer_scan` runs these queries in `BatchedBindingCollector`.
        ConsumerQuery::MemberBinding { .. } | ConsumerQuery::StyleXThemeGroup { .. } => {}
        ConsumerQuery::PandaTokenCall { alias, leaf_paths } => {
            if alias.is_empty() || leaf_paths.is_empty() {
                return;
            }
            let mut collector = PandaTokenCallCollector {
                lines: LineCounter::new(source),
                alias,
                leaf_paths,
                hits: Vec::new(),
            };
            collector.visit_program(program);
            out.extend(collector.hits.into_iter().map(|hit| (idx, hit)));
        }
        ConsumerQuery::PandaStyleValues {
            aliases,
            leaf_paths,
        } => {
            if aliases.is_empty() || leaf_paths.is_empty() {
                return;
            }
            let mut collector = PandaStyleValueCollector {
                lines: LineCounter::new(source),
                aliases,
                leaf_paths,
                hits: Vec::new(),
            };
            collector.visit_program(program);
            out.extend(collector.hits.into_iter().map(|hit| (idx, hit)));
        }
        ConsumerQuery::ThemeReads { leaf_paths } => {
            if leaf_paths.is_empty() {
                return;
            }
            let mut collector = ThemeConsumerCollector {
                lines: LineCounter::new(source),
                leaf_paths,
                hits: Vec::new(),
            };
            collector.visit_program(program);
            out.extend(collector.hits.into_iter().map(|hit| (idx, hit)));
        }
    }
}

struct PandaTokenCallCollector<'a, 'b> {
    lines: LineCounter<'a>,
    alias: &'b str,
    leaf_paths: &'b FxHashSet<String>,
    hits: Vec<TokenConsumerHit>,
}

impl<'a> Visit<'a> for PandaTokenCallCollector<'a, '_> {
    fn visit_call_expression(&mut self, call: &oxc_ast::ast::CallExpression<'a>) {
        let Expression::Identifier(callee) = &call.callee else {
            walk::walk_call_expression(self, call);
            return;
        };
        if callee.name.as_str() == self.alias
            && let Some(Argument::StringLiteral(lit)) = call.arguments.first()
        {
            let token_path = lit.value.as_str();
            if self.leaf_paths.contains(token_path) {
                let line = self.lines.line_at(call.span().start);
                self.hits.push(TokenConsumerHit {
                    token_path: token_path.to_owned(),
                    line,
                });
            }
        }
        walk::walk_call_expression(self, call);
    }
}

struct PandaStyleValueCollector<'a, 'b> {
    lines: LineCounter<'a>,
    aliases: &'b FxHashSet<String>,
    leaf_paths: &'b FxHashSet<String>,
    hits: Vec<TokenConsumerHit>,
}

impl<'a> PandaStyleValueCollector<'a, '_> {
    fn record_object(&mut self, obj: &ObjectExpression<'a>) {
        for prop in &obj.properties {
            let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                continue;
            };
            self.record_expression(&prop.value);
        }
    }

    fn record_expression(&mut self, expr: &Expression<'a>) {
        match expr {
            Expression::StringLiteral(lit) => {
                let token_path = lit.value.as_str();
                if self.leaf_paths.contains(token_path) {
                    let line = self.lines.line_at(lit.span().start);
                    self.hits.push(TokenConsumerHit {
                        token_path: token_path.to_owned(),
                        line,
                    });
                }
            }
            Expression::ObjectExpression(obj) => self.record_object(obj),
            _ => {}
        }
    }
}

impl<'a> Visit<'a> for PandaStyleValueCollector<'a, '_> {
    fn visit_call_expression(&mut self, call: &oxc_ast::ast::CallExpression<'a>) {
        let Expression::Identifier(callee) = &call.callee else {
            walk::walk_call_expression(self, call);
            return;
        };
        if self.aliases.contains(callee.name.as_str()) {
            for arg in &call.arguments {
                if let Argument::ObjectExpression(obj) = arg {
                    self.record_object(obj);
                }
            }
        }
        walk::walk_call_expression(self, call);
    }
}

struct ThemeDefCollector<'a> {
    lines: LineCounter<'a>,
    defs: Vec<CssInJsTokenDef>,
}

impl<'a> ThemeDefCollector<'a> {
    fn process_declarator(&mut self, decl: &VariableDeclarator<'a>) {
        let BindingPattern::BindingIdentifier(binding) = &decl.id else {
            return;
        };
        let binding_name = binding.name.as_str();
        if !is_theme_binding_name(binding_name) {
            return;
        }
        let Some(Expression::ObjectExpression(obj)) = &decl.init else {
            return;
        };
        let mut tokens = Vec::new();
        collect_token_leaves(
            &mut self.lines,
            obj,
            "",
            CssInJsTokenOrigin::Theme,
            &mut tokens,
        );
        if tokens.is_empty() {
            return;
        }
        self.defs.push(CssInJsTokenDef {
            binding: binding_name.to_owned(),
            origin: CssInJsTokenOrigin::Theme,
            tokens,
        });
    }
}

impl<'a> Visit<'a> for ThemeDefCollector<'a> {
    fn visit_variable_declarator(&mut self, decl: &VariableDeclarator<'a>) {
        self.process_declarator(decl);
        walk::walk_variable_declarator(self, decl);
    }
}

struct ThemeConsumerCollector<'a, 'b> {
    lines: LineCounter<'a>,
    leaf_paths: &'b FxHashSet<String>,
    hits: Vec<TokenConsumerHit>,
}

impl<'a> ThemeConsumerCollector<'a, '_> {
    fn record(&mut self, chain: Option<(&'a str, Vec<String>)>, span_start: u32) {
        let Some((base, segments)) = chain else {
            return;
        };
        let token_segments: &[String] = match base {
            "theme" => &segments,
            "props" if segments.first().is_some_and(|segment| segment == "theme") => &segments[1..],
            _ => return,
        };
        if token_segments.is_empty() {
            return;
        }
        let token_path = token_segments.join(".");
        if self.leaf_paths.contains(&token_path) {
            let line = self.lines.line_at(span_start);
            self.hits.push(TokenConsumerHit { token_path, line });
        }
    }
}

impl<'a> Visit<'a> for ThemeConsumerCollector<'a, '_> {
    fn visit_static_member_expression(&mut self, member: &StaticMemberExpression<'a>) {
        let mut chain = access_object_chain(&member.object);
        if let Some((_, segments)) = chain.as_mut() {
            segments.push(member.property.name.to_string());
        }
        self.record(chain, member.span().start);
        walk::walk_static_member_expression(self, member);
    }

    fn visit_computed_member_expression(&mut self, member: &ComputedMemberExpression<'a>) {
        let mut chain = access_object_chain(&member.object);
        if let (Some((_, segments)), Some(key)) =
            (chain.as_mut(), static_computed_key(&member.expression))
        {
            segments.push(key);
        } else {
            chain = None;
        }
        self.record(chain, member.span().start);
        walk::walk_computed_member_expression(self, member);
    }
}

/// Reconstruct the `(base identifier, [segments])` chain of a member-access OBJECT
/// expression, threading through both static (`a.b`) and string-literal-computed
/// (`a['b']`) member access. `vars.color` -> `("vars", ["color"])`. Returns `None`
/// if the chain is not rooted at a plain identifier (a call result, `this`, a
/// non-literal computed key, etc.).
fn access_object_chain<'a>(expr: &Expression<'a>) -> Option<(&'a str, Vec<String>)> {
    match expr {
        Expression::Identifier(id) => Some((id.name.as_str(), Vec::new())),
        Expression::StaticMemberExpression(inner) => {
            let (base, mut segments) = access_object_chain(&inner.object)?;
            segments.push(inner.property.name.to_string());
            Some((base, segments))
        }
        Expression::ComputedMemberExpression(inner) => {
            let (base, mut segments) = access_object_chain(&inner.object)?;
            segments.push(static_computed_key(&inner.expression)?);
            Some((base, segments))
        }
        Expression::ParenthesizedExpression(expression) => {
            access_object_chain(&expression.expression)
        }
        Expression::TSAsExpression(expression) => access_object_chain(&expression.expression),
        Expression::TSSatisfiesExpression(expression) => {
            access_object_chain(&expression.expression)
        }
        Expression::TSNonNullExpression(expression) => access_object_chain(&expression.expression),
        Expression::TSTypeAssertion(expression) => access_object_chain(&expression.expression),
        _ => None,
    }
}

fn binding_access_object_chain<'a>(expr: &Expression<'a>) -> Option<(&'a str, Span, Vec<String>)> {
    match expr {
        Expression::Identifier(id) => Some((id.name.as_str(), id.span, Vec::new())),
        Expression::StaticMemberExpression(inner) => {
            let (base, span, mut segments) = binding_access_object_chain(&inner.object)?;
            segments.push(inner.property.name.to_string());
            Some((base, span, segments))
        }
        Expression::ComputedMemberExpression(inner) => {
            let (base, span, mut segments) = binding_access_object_chain(&inner.object)?;
            segments.push(static_computed_key(&inner.expression)?);
            Some((base, span, segments))
        }
        Expression::ParenthesizedExpression(expression) => {
            binding_access_object_chain(&expression.expression)
        }
        Expression::TSAsExpression(expression) => {
            binding_access_object_chain(&expression.expression)
        }
        Expression::TSSatisfiesExpression(expression) => {
            binding_access_object_chain(&expression.expression)
        }
        Expression::TSNonNullExpression(expression) => {
            binding_access_object_chain(&expression.expression)
        }
        Expression::TSTypeAssertion(expression) => {
            binding_access_object_chain(&expression.expression)
        }
        _ => None,
    }
}

fn unwrap_transparent_expression<'a, 'b: 'a>(mut expr: &'a Expression<'b>) -> &'a Expression<'b> {
    loop {
        expr = match expr {
            Expression::ParenthesizedExpression(expression) => &expression.expression,
            Expression::TSAsExpression(expression) => &expression.expression,
            Expression::TSSatisfiesExpression(expression) => &expression.expression,
            Expression::TSNonNullExpression(expression) => &expression.expression,
            Expression::TSTypeAssertion(expression) => &expression.expression,
            _ => return expr,
        };
    }
}

fn call_receiver_root<'a, 'b: 'a>(callee: &'a Expression<'b>) -> Option<RootBinding<'a>> {
    match callee {
        Expression::StaticMemberExpression(member) => expression_root_binding(&member.object),
        Expression::ComputedMemberExpression(member) => expression_root_binding(&member.object),
        Expression::ParenthesizedExpression(expression) => {
            call_receiver_root(&expression.expression)
        }
        Expression::TSAsExpression(expression) => call_receiver_root(&expression.expression),
        Expression::TSSatisfiesExpression(expression) => call_receiver_root(&expression.expression),
        Expression::TSNonNullExpression(expression) => call_receiver_root(&expression.expression),
        Expression::TSTypeAssertion(expression) => call_receiver_root(&expression.expression),
        _ => None,
    }
}

fn call_receiver_expression<'a, 'b: 'a>(callee: &'a Expression<'b>) -> Option<&'a Expression<'b>> {
    match callee {
        Expression::StaticMemberExpression(member) => Some(&member.object),
        Expression::ComputedMemberExpression(member) => Some(&member.object),
        Expression::ParenthesizedExpression(expression) => {
            call_receiver_expression(&expression.expression)
        }
        Expression::TSAsExpression(expression) => call_receiver_expression(&expression.expression),
        Expression::TSSatisfiesExpression(expression) => {
            call_receiver_expression(&expression.expression)
        }
        Expression::TSNonNullExpression(expression) => {
            call_receiver_expression(&expression.expression)
        }
        Expression::TSTypeAssertion(expression) => call_receiver_expression(&expression.expression),
        _ => None,
    }
}

fn is_static_stylex_theme_override<'a>(
    expression: &'a Expression<'a>,
    before: u32,
    static_values: &FxHashMap<&'a str, (u32, &'a Expression<'a>)>,
    imports: &FxHashMap<&'a str, (Lib, &'a str)>,
    root_reference_spans: &FxHashSet<Span>,
    visiting: &mut FxHashSet<&'a str>,
) -> bool {
    let expression = unwrap_transparent_expression(expression);
    if let Some(is_static) = is_static_stylex_composite_expression(
        expression,
        before,
        static_values,
        imports,
        root_reference_spans,
        visiting,
    ) {
        return is_static;
    }
    match expression {
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_) => true,
        Expression::ObjectExpression(object) => is_static_stylex_theme_object(
            object,
            before,
            static_values,
            imports,
            root_reference_spans,
            visiting,
        ),
        Expression::Identifier(identifier) => {
            if !root_reference_spans.contains(&identifier.span) {
                return false;
            }
            let name = identifier.name.as_str();
            let Some(&(declaration_start, value)) = static_values.get(name) else {
                return false;
            };
            if declaration_start >= before || !visiting.insert(name) {
                return false;
            }
            let is_static = is_static_stylex_theme_override(
                value,
                declaration_start,
                static_values,
                imports,
                root_reference_spans,
                visiting,
            );
            visiting.remove(name);
            is_static
        }
        Expression::StaticMemberExpression(_) | Expression::ComputedMemberExpression(_) => {
            let mut member_visiting = visiting.clone();
            let resolver = StyleXStaticResolver {
                before,
                static_values,
                root_reference_spans,
            };
            resolve_static_stylex_member(expression, &resolver, &mut member_visiting).is_some_and(
                |resolved| {
                    is_static_stylex_theme_override(
                        resolved.value,
                        resolved.before,
                        static_values,
                        imports,
                        root_reference_spans,
                        &mut member_visiting,
                    )
                },
            )
        }
        Expression::CallExpression(call)
            if call.arguments.len() == 1
                && is_root_stylex_static_call(&call.callee, imports, root_reference_spans) =>
        {
            call.arguments.first().is_some_and(|argument| {
                argument.as_expression().is_some_and(|argument| {
                    is_static_stylex_theme_override(
                        argument,
                        before,
                        static_values,
                        imports,
                        root_reference_spans,
                        visiting,
                    )
                })
            })
        }
        Expression::CallExpression(call) => is_static_stylex_pure_call(
            call,
            before,
            static_values,
            imports,
            root_reference_spans,
            visiting,
        ),
        _ => false,
    }
}

fn is_static_stylex_composite_expression<'a>(
    expression: &'a Expression<'a>,
    before: u32,
    static_values: &FxHashMap<&'a str, (u32, &'a Expression<'a>)>,
    imports: &FxHashMap<&'a str, (Lib, &'a str)>,
    root_reference_spans: &FxHashSet<Span>,
    visiting: &mut FxHashSet<&'a str>,
) -> Option<bool> {
    let mut check = |expression| {
        is_static_stylex_theme_override(
            expression,
            before,
            static_values,
            imports,
            root_reference_spans,
            visiting,
        )
    };
    match expression {
        Expression::TemplateLiteral(template) => Some(template.expressions.iter().all(check)),
        Expression::TaggedTemplateExpression(tagged) => Some(
            is_root_string_raw_tag(&tagged.tag, static_values, root_reference_spans)
                && tagged.quasi.expressions.iter().all(check),
        ),
        Expression::UnaryExpression(unary) => Some(
            !unary.operator.is_delete()
                && (unary.operator != UnaryOperator::UnaryPlus
                    || !is_static_stylex_bigint_value(
                        &unary.argument,
                        before,
                        static_values,
                        root_reference_spans,
                        &mut FxHashSet::default(),
                    ))
                && check(&unary.argument),
        ),
        Expression::ConditionalExpression(conditional) => Some(
            check(&conditional.test)
                && check(&conditional.consequent)
                && check(&conditional.alternate),
        ),
        Expression::LogicalExpression(logical) => {
            Some(check(&logical.left) && check(&logical.right))
        }
        Expression::SequenceExpression(sequence) => Some(sequence.expressions.iter().all(check)),
        Expression::BinaryExpression(binary) => Some(
            !binary.operator.is_relational()
                && (!binary.operator.is_numeric_or_string_binary_operator()
                    || (!is_static_stylex_bigint_value(
                        &binary.left,
                        before,
                        static_values,
                        root_reference_spans,
                        &mut FxHashSet::default(),
                    ) && !is_static_stylex_bigint_value(
                        &binary.right,
                        before,
                        static_values,
                        root_reference_spans,
                        &mut FxHashSet::default(),
                    )))
                && check(&binary.left)
                && check(&binary.right),
        ),
        _ => None,
    }
}

fn is_static_stylex_theme_object<'a>(
    object: &'a ObjectExpression<'a>,
    before: u32,
    static_values: &FxHashMap<&'a str, (u32, &'a Expression<'a>)>,
    imports: &FxHashMap<&'a str, (Lib, &'a str)>,
    root_reference_spans: &FxHashSet<Span>,
    visiting: &mut FxHashSet<&'a str>,
) -> bool {
    object.properties.iter().all(|property| match property {
        ObjectPropertyKind::ObjectProperty(property) => {
            if property.computed {
                let Some(key) = property.key.as_expression() else {
                    return false;
                };
                if !is_static_stylex_computed_key(
                    key,
                    before,
                    static_values,
                    imports,
                    root_reference_spans,
                    visiting,
                ) {
                    return false;
                }
            }
            is_static_stylex_theme_override(
                &property.value,
                before,
                static_values,
                imports,
                root_reference_spans,
                visiting,
            )
        }
        ObjectPropertyKind::SpreadProperty(spread) => is_static_stylex_theme_override_object(
            &spread.argument,
            before,
            static_values,
            imports,
            root_reference_spans,
            visiting,
        ),
    })
}

fn is_static_stylex_computed_key<'a>(
    expression: &'a Expression<'a>,
    before: u32,
    static_values: &FxHashMap<&'a str, (u32, &'a Expression<'a>)>,
    imports: &FxHashMap<&'a str, (Lib, &'a str)>,
    root_reference_spans: &FxHashSet<Span>,
    visiting: &mut FxHashSet<&'a str>,
) -> bool {
    match unwrap_transparent_expression(expression) {
        Expression::StringLiteral(_) | Expression::NumericLiteral(_) => true,
        Expression::TemplateLiteral(template) => template.expressions.iter().all(|expression| {
            is_static_stylex_theme_override(
                expression,
                before,
                static_values,
                imports,
                root_reference_spans,
                visiting,
            )
        }),
        Expression::BinaryExpression(binary)
            if binary.operator.is_numeric_or_string_binary_operator() =>
        {
            is_static_stylex_theme_override(
                expression,
                before,
                static_values,
                imports,
                root_reference_spans,
                visiting,
            )
        }
        Expression::Identifier(identifier) => {
            if !root_reference_spans.contains(&identifier.span) {
                return false;
            }
            let name = identifier.name.as_str();
            let Some(&(declaration_start, value)) = static_values.get(name) else {
                return false;
            };
            if declaration_start >= before || !visiting.insert(name) {
                return false;
            }
            let is_static = is_static_stylex_computed_key(
                value,
                declaration_start,
                static_values,
                imports,
                root_reference_spans,
                visiting,
            );
            visiting.remove(name);
            is_static
        }
        Expression::StaticMemberExpression(_) | Expression::ComputedMemberExpression(_) => {
            let mut member_visiting = visiting.clone();
            let resolver = StyleXStaticResolver {
                before,
                static_values,
                root_reference_spans,
            };
            resolve_static_stylex_member(expression, &resolver, &mut member_visiting).is_some_and(
                |resolved| {
                    is_static_stylex_computed_key(
                        resolved.value,
                        resolved.before,
                        static_values,
                        imports,
                        root_reference_spans,
                        &mut member_visiting,
                    )
                },
            )
        }
        _ => false,
    }
}

struct StyleXStaticResolver<'maps, 'ast> {
    before: u32,
    static_values: &'maps FxHashMap<&'ast str, (u32, &'ast Expression<'ast>)>,
    root_reference_spans: &'maps FxHashSet<Span>,
}

struct ResolvedStyleXStaticMember<'ast> {
    value: &'ast Expression<'ast>,
    before: u32,
}

#[derive(Clone, Copy)]
struct ResolvedStyleXStaticObject<'ast> {
    object: &'ast ObjectExpression<'ast>,
    before: u32,
}

fn resolve_static_stylex_member<'a>(
    expression: &'a Expression<'a>,
    resolver: &StyleXStaticResolver<'_, 'a>,
    visiting: &mut FxHashSet<&'a str>,
) -> Option<ResolvedStyleXStaticMember<'a>> {
    match unwrap_transparent_expression(expression) {
        Expression::StaticMemberExpression(member) => {
            let object = resolve_static_stylex_object(&member.object, resolver, visiting)?;
            resolve_static_stylex_object_property(
                object,
                member.property.name.as_str(),
                resolver,
                visiting,
            )
        }
        Expression::ComputedMemberExpression(member) => {
            let object = resolve_static_stylex_object(&member.object, resolver, visiting)?;
            let key = resolve_static_stylex_member_key(&member.expression, resolver, visiting)?;
            resolve_static_stylex_object_property(object, &key, resolver, visiting)
        }
        _ => None,
    }
}

fn resolve_static_stylex_object<'a>(
    expression: &'a Expression<'a>,
    resolver: &StyleXStaticResolver<'_, 'a>,
    visiting: &mut FxHashSet<&'a str>,
) -> Option<ResolvedStyleXStaticObject<'a>> {
    match unwrap_transparent_expression(expression) {
        Expression::ObjectExpression(object) => Some(ResolvedStyleXStaticObject {
            object,
            before: resolver.before,
        }),
        Expression::Identifier(identifier) => {
            if !resolver.root_reference_spans.contains(&identifier.span) {
                return None;
            }
            let name = identifier.name.as_str();
            let &(declaration_start, value) = resolver.static_values.get(name)?;
            if declaration_start >= resolver.before || !visiting.insert(name) {
                return None;
            }
            let nested_resolver = StyleXStaticResolver {
                before: declaration_start,
                static_values: resolver.static_values,
                root_reference_spans: resolver.root_reference_spans,
            };
            resolve_static_stylex_object(value, &nested_resolver, visiting)
        }
        Expression::StaticMemberExpression(_) | Expression::ComputedMemberExpression(_) => {
            let resolved = resolve_static_stylex_member(expression, resolver, visiting)?;
            let nested_resolver = StyleXStaticResolver {
                before: resolved.before,
                static_values: resolver.static_values,
                root_reference_spans: resolver.root_reference_spans,
            };
            resolve_static_stylex_object(resolved.value, &nested_resolver, visiting)
        }
        _ => None,
    }
}

fn resolve_static_stylex_object_property<'a>(
    resolved_object: ResolvedStyleXStaticObject<'a>,
    wanted: &str,
    resolver: &StyleXStaticResolver<'_, 'a>,
    visiting: &mut FxHashSet<&'a str>,
) -> Option<ResolvedStyleXStaticMember<'a>> {
    for property in resolved_object.object.properties.iter().rev() {
        match property {
            ObjectPropertyKind::ObjectProperty(property) => {
                let key = if property.computed {
                    resolve_static_stylex_member_key(
                        property.key.as_expression()?,
                        resolver,
                        visiting,
                    )?
                } else {
                    property.key.static_name()?.to_string()
                };
                if key == wanted {
                    return Some(ResolvedStyleXStaticMember {
                        value: &property.value,
                        before: resolved_object.before,
                    });
                }
            }
            ObjectPropertyKind::SpreadProperty(spread) => {
                let spread_resolver = StyleXStaticResolver {
                    before: resolved_object.before,
                    static_values: resolver.static_values,
                    root_reference_spans: resolver.root_reference_spans,
                };
                let spread_object =
                    resolve_static_stylex_object(&spread.argument, &spread_resolver, visiting)?;
                if let Some(value) =
                    resolve_static_stylex_object_property(spread_object, wanted, resolver, visiting)
                {
                    return Some(value);
                }
            }
        }
    }
    None
}

fn resolve_static_stylex_member_key<'a>(
    expression: &'a Expression<'a>,
    resolver: &StyleXStaticResolver<'_, 'a>,
    visiting: &mut FxHashSet<&'a str>,
) -> Option<String> {
    if let Some(key) = static_computed_key(expression) {
        return Some(key);
    }
    let Expression::Identifier(identifier) = unwrap_transparent_expression(expression) else {
        return None;
    };
    if !resolver.root_reference_spans.contains(&identifier.span) {
        return None;
    }
    let name = identifier.name.as_str();
    let &(declaration_start, value) = resolver.static_values.get(name)?;
    if declaration_start >= resolver.before || !visiting.insert(name) {
        return None;
    }
    let nested_resolver = StyleXStaticResolver {
        before: declaration_start,
        static_values: resolver.static_values,
        root_reference_spans: resolver.root_reference_spans,
    };
    let key = resolve_static_stylex_member_key(value, &nested_resolver, visiting);
    visiting.remove(name);
    key
}

fn is_static_stylex_theme_override_object<'a>(
    expression: &'a Expression<'a>,
    before: u32,
    static_values: &FxHashMap<&'a str, (u32, &'a Expression<'a>)>,
    imports: &FxHashMap<&'a str, (Lib, &'a str)>,
    root_reference_spans: &FxHashSet<Span>,
    visiting: &mut FxHashSet<&'a str>,
) -> bool {
    match unwrap_transparent_expression(expression) {
        Expression::ObjectExpression(_) => is_static_stylex_theme_override(
            expression,
            before,
            static_values,
            imports,
            root_reference_spans,
            visiting,
        ),
        Expression::Identifier(identifier) => {
            if !root_reference_spans.contains(&identifier.span) {
                return false;
            }
            let name = identifier.name.as_str();
            let Some(&(declaration_start, value)) = static_values.get(name) else {
                return false;
            };
            if declaration_start >= before || !visiting.insert(name) {
                return false;
            }
            let is_static = is_static_stylex_theme_override_object(
                value,
                declaration_start,
                static_values,
                imports,
                root_reference_spans,
                visiting,
            );
            visiting.remove(name);
            is_static
        }
        Expression::StaticMemberExpression(_) | Expression::ComputedMemberExpression(_) => {
            let mut member_visiting = visiting.clone();
            let resolver = StyleXStaticResolver {
                before,
                static_values,
                root_reference_spans,
            };
            resolve_static_stylex_member(expression, &resolver, &mut member_visiting).is_some_and(
                |resolved| {
                    is_static_stylex_theme_override_object(
                        resolved.value,
                        resolved.before,
                        static_values,
                        imports,
                        root_reference_spans,
                        &mut member_visiting,
                    )
                },
            )
        }
        _ => false,
    }
}

fn is_root_string_raw_tag(
    tag: &Expression<'_>,
    static_values: &FxHashMap<&str, (u32, &Expression<'_>)>,
    root_reference_spans: &FxHashSet<Span>,
) -> bool {
    let Expression::StaticMemberExpression(member) = unwrap_transparent_expression(tag) else {
        return false;
    };
    let Expression::Identifier(object) = unwrap_transparent_expression(&member.object) else {
        return false;
    };
    object.name == "String"
        && member.property.name == "raw"
        && root_reference_spans.contains(&object.span)
        && !static_values.contains_key("String")
}

fn is_static_stylex_pure_call<'a>(
    call: &'a CallExpression<'a>,
    before: u32,
    static_values: &FxHashMap<&'a str, (u32, &'a Expression<'a>)>,
    imports: &FxHashMap<&'a str, (Lib, &'a str)>,
    root_reference_spans: &FxHashSet<Span>,
    visiting: &mut FxHashSet<&'a str>,
) -> bool {
    if is_static_stylex_local_arrow_call(
        call,
        before,
        static_values,
        imports,
        root_reference_spans,
        visiting,
    ) {
        return true;
    }
    let allow_array_arguments =
        is_root_object_from_entries_call(&call.callee, static_values, root_reference_spans);
    if allow_array_arguments {
        return is_static_stylex_entries_call(
            call,
            before,
            static_values,
            imports,
            root_reference_spans,
            visiting,
        );
    }
    if !is_root_scalar_pure_call(
        &call.callee,
        before,
        static_values,
        imports,
        root_reference_spans,
        visiting,
    ) || !is_static_stylex_scalar_call_shape(call, before, static_values, root_reference_spans)
    {
        return false;
    }
    if is_root_math_call(&call.callee, static_values, root_reference_spans)
        && call.arguments.iter().any(|argument| {
            argument.as_expression().is_none_or(|argument| {
                is_static_stylex_bigint_value(
                    argument,
                    before,
                    static_values,
                    root_reference_spans,
                    &mut FxHashSet::default(),
                )
            })
        })
    {
        return false;
    }
    call.arguments.iter().all(|argument| {
        argument.as_expression().is_some_and(|argument| {
            is_definitely_static_primitive(
                argument,
                before,
                static_values,
                root_reference_spans,
                visiting,
            ) && is_static_stylex_theme_override(
                argument,
                before,
                static_values,
                imports,
                root_reference_spans,
                visiting,
            )
        })
    })
}

fn is_root_math_call(
    callee: &Expression<'_>,
    static_values: &FxHashMap<&str, (u32, &Expression<'_>)>,
    root_reference_spans: &FxHashSet<Span>,
) -> bool {
    let Expression::StaticMemberExpression(member) = unwrap_transparent_expression(callee) else {
        return false;
    };
    let Expression::Identifier(object) = unwrap_transparent_expression(&member.object) else {
        return false;
    };
    object.name == "Math"
        && root_reference_spans.contains(&object.span)
        && !static_values.contains_key("Math")
}

fn is_definitely_static_primitive<'a>(
    expression: &'a Expression<'a>,
    before: u32,
    static_values: &FxHashMap<&'a str, (u32, &'a Expression<'a>)>,
    root_reference_spans: &FxHashSet<Span>,
    visiting: &mut FxHashSet<&'a str>,
) -> bool {
    match unwrap_transparent_expression(expression) {
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_) => true,
        Expression::TemplateLiteral(template) => template.expressions.iter().all(|expression| {
            is_definitely_static_primitive(
                expression,
                before,
                static_values,
                root_reference_spans,
                visiting,
            )
        }),
        Expression::UnaryExpression(unary) => {
            !unary.operator.is_delete()
                && is_definitely_static_primitive(
                    &unary.argument,
                    before,
                    static_values,
                    root_reference_spans,
                    visiting,
                )
        }
        Expression::BinaryExpression(binary) => is_static_stylex_primitive_pair(
            &binary.left,
            &binary.right,
            before,
            static_values,
            root_reference_spans,
            visiting,
        ),
        Expression::LogicalExpression(logical) => is_static_stylex_primitive_pair(
            &logical.left,
            &logical.right,
            before,
            static_values,
            root_reference_spans,
            visiting,
        ),
        Expression::ConditionalExpression(conditional) => is_static_stylex_primitive_pair(
            &conditional.consequent,
            &conditional.alternate,
            before,
            static_values,
            root_reference_spans,
            visiting,
        ),
        Expression::SequenceExpression(sequence) => {
            sequence.expressions.last().is_some_and(|expression| {
                is_definitely_static_primitive(
                    expression,
                    before,
                    static_values,
                    root_reference_spans,
                    visiting,
                )
            })
        }
        Expression::Identifier(identifier) => {
            if !root_reference_spans.contains(&identifier.span) {
                return false;
            }
            let name = identifier.name.as_str();
            let Some(&(declaration_start, value)) = static_values.get(name) else {
                return false;
            };
            if declaration_start >= before || !visiting.insert(name) {
                return false;
            }
            let is_primitive = is_definitely_static_primitive(
                value,
                declaration_start,
                static_values,
                root_reference_spans,
                visiting,
            );
            visiting.remove(name);
            is_primitive
        }
        _ => false,
    }
}

fn is_static_stylex_primitive_pair<'a>(
    left: &'a Expression<'a>,
    right: &'a Expression<'a>,
    before: u32,
    static_values: &FxHashMap<&'a str, (u32, &'a Expression<'a>)>,
    root_reference_spans: &FxHashSet<Span>,
    visiting: &mut FxHashSet<&'a str>,
) -> bool {
    is_definitely_static_primitive(left, before, static_values, root_reference_spans, visiting)
        && is_definitely_static_primitive(
            right,
            before,
            static_values,
            root_reference_spans,
            visiting,
        )
}

fn is_static_stylex_bigint_value<'a>(
    expression: &'a Expression<'a>,
    before: u32,
    static_values: &FxHashMap<&'a str, (u32, &'a Expression<'a>)>,
    root_reference_spans: &FxHashSet<Span>,
    visiting: &mut FxHashSet<&'a str>,
) -> bool {
    let parameters = FxHashMap::default();
    let context = StyleXBigIntContext {
        before,
        static_values,
        root_reference_spans,
        parameters: &parameters,
    };
    is_static_stylex_bigint_value_with_context(expression, &context, visiting)
}

#[derive(Clone, Copy)]
struct StyleXBigIntContext<'maps, 'ast> {
    before: u32,
    static_values: &'maps FxHashMap<&'ast str, (u32, &'ast Expression<'ast>)>,
    root_reference_spans: &'maps FxHashSet<Span>,
    parameters: &'maps FxHashMap<&'ast str, &'ast Expression<'ast>>,
}

fn is_static_stylex_bigint_value_with_context<'a>(
    expression: &'a Expression<'a>,
    context: &StyleXBigIntContext<'_, 'a>,
    visiting: &mut FxHashSet<&'a str>,
) -> bool {
    match unwrap_transparent_expression(expression) {
        Expression::BigIntLiteral(_) => true,
        Expression::Identifier(identifier) => {
            let name = identifier.name.as_str();
            if let Some(value) = context.parameters.get(name) {
                return is_static_stylex_bigint_value_with_context(value, context, visiting);
            }
            if !context.root_reference_spans.contains(&identifier.span) {
                return false;
            }
            let Some(&(declaration_start, value)) = context.static_values.get(name) else {
                return false;
            };
            if declaration_start >= context.before || !visiting.insert(name) {
                return false;
            }
            let nested_context = StyleXBigIntContext {
                before: declaration_start,
                ..*context
            };
            let is_bigint =
                is_static_stylex_bigint_value_with_context(value, &nested_context, visiting);
            visiting.remove(name);
            is_bigint
        }
        Expression::UnaryExpression(unary)
            if matches!(
                unary.operator,
                UnaryOperator::UnaryNegation | UnaryOperator::BitwiseNot
            ) =>
        {
            is_static_stylex_bigint_value_with_context(&unary.argument, context, visiting)
        }
        Expression::BinaryExpression(binary)
            if binary.operator.is_numeric_or_string_binary_operator() =>
        {
            is_static_stylex_bigint_pair(&binary.left, &binary.right, context, visiting)
        }
        Expression::ConditionalExpression(conditional) => is_static_stylex_bigint_pair(
            &conditional.consequent,
            &conditional.alternate,
            context,
            visiting,
        ),
        Expression::LogicalExpression(logical) => {
            is_static_stylex_bigint_pair(&logical.left, &logical.right, context, visiting)
        }
        Expression::SequenceExpression(sequence) => {
            sequence.expressions.last().is_some_and(|expression| {
                is_static_stylex_bigint_value_with_context(expression, context, visiting)
            })
        }
        Expression::StaticMemberExpression(_) | Expression::ComputedMemberExpression(_) => {
            let mut member_visiting = visiting.clone();
            let resolver = StyleXStaticResolver {
                before: context.before,
                static_values: context.static_values,
                root_reference_spans: context.root_reference_spans,
            };
            resolve_static_stylex_member(expression, &resolver, &mut member_visiting).is_some_and(
                |resolved| {
                    let nested_context = StyleXBigIntContext {
                        before: resolved.before,
                        ..*context
                    };
                    is_static_stylex_bigint_value_with_context(
                        resolved.value,
                        &nested_context,
                        &mut member_visiting,
                    )
                },
            )
        }
        Expression::CallExpression(call) => {
            is_static_stylex_bigint_arrow_call(call, context, visiting)
        }
        _ => false,
    }
}

fn is_static_stylex_bigint_pair<'a>(
    left: &'a Expression<'a>,
    right: &'a Expression<'a>,
    context: &StyleXBigIntContext<'_, 'a>,
    visiting: &mut FxHashSet<&'a str>,
) -> bool {
    is_static_stylex_bigint_value_with_context(left, context, visiting)
        || is_static_stylex_bigint_value_with_context(right, context, visiting)
}

fn is_static_stylex_bigint_arrow_call<'a>(
    call: &'a CallExpression<'a>,
    context: &StyleXBigIntContext<'_, 'a>,
    visiting: &mut FxHashSet<&'a str>,
) -> bool {
    let Expression::Identifier(callee) = unwrap_transparent_expression(&call.callee) else {
        return false;
    };
    if !context.root_reference_spans.contains(&callee.span) {
        return false;
    }
    let name = callee.name.as_str();
    let Some(&(declaration_start, value)) = context.static_values.get(name) else {
        return false;
    };
    let Expression::ArrowFunctionExpression(arrow) = unwrap_transparent_expression(value) else {
        return false;
    };
    if declaration_start >= context.before
        || arrow.r#async
        || arrow.params.rest.is_some()
        || arrow.params.items.len() != call.arguments.len()
        || !visiting.insert(name)
    {
        return false;
    }
    let Some(body) = stylex_arrow_expression_body(arrow) else {
        visiting.remove(name);
        return false;
    };
    let mut arrow_parameters = context.parameters.clone();
    for (parameter, argument) in arrow.params.items.iter().zip(&call.arguments) {
        let (BindingPattern::BindingIdentifier(binding), Some(argument)) =
            (&parameter.pattern, argument.as_expression())
        else {
            visiting.remove(name);
            return false;
        };
        arrow_parameters.insert(binding.name.as_str(), argument);
    }
    let nested_context = StyleXBigIntContext {
        before: context.before,
        parameters: &arrow_parameters,
        ..*context
    };
    let is_bigint = is_static_stylex_bigint_value_with_context(body, &nested_context, visiting);
    visiting.remove(name);
    is_bigint
}

fn is_root_object_from_entries_call(
    callee: &Expression<'_>,
    static_values: &FxHashMap<&str, (u32, &Expression<'_>)>,
    root_reference_spans: &FxHashSet<Span>,
) -> bool {
    let Expression::StaticMemberExpression(member) = unwrap_transparent_expression(callee) else {
        return false;
    };
    let Expression::Identifier(object) = unwrap_transparent_expression(&member.object) else {
        return false;
    };
    object.name == "Object"
        && member.property.name == "fromEntries"
        && root_reference_spans.contains(&object.span)
        && !static_values.contains_key("Object")
}

fn is_root_scalar_pure_call<'a>(
    callee: &'a Expression<'a>,
    before: u32,
    static_values: &FxHashMap<&'a str, (u32, &'a Expression<'a>)>,
    imports: &FxHashMap<&'a str, (Lib, &'a str)>,
    root_reference_spans: &FxHashSet<Span>,
    visiting: &mut FxHashSet<&'a str>,
) -> bool {
    match unwrap_transparent_expression(callee) {
        Expression::Identifier(identifier) => {
            matches!(identifier.name.as_str(), "String" | "Number")
                && root_reference_spans.contains(&identifier.span)
                && !static_values.contains_key(identifier.name.as_str())
        }
        Expression::StaticMemberExpression(member) => {
            let method = member.property.name.as_str();
            match unwrap_transparent_expression(&member.object) {
                Expression::Identifier(object) if object.name == "Math" => {
                    root_reference_spans.contains(&object.span)
                        && !static_values.contains_key("Math")
                        && is_static_math_method(method)
                }
                object => match static_stylex_scalar_kind(
                    object,
                    before,
                    static_values,
                    imports,
                    root_reference_spans,
                    visiting,
                ) {
                    Some(StyleXScalarKind::String) => is_static_string_method(method),
                    Some(StyleXScalarKind::Number) => is_static_number_method(method),
                    Some(StyleXScalarKind::Other) | None => false,
                },
            }
        }
        _ => false,
    }
}

#[derive(Clone, Copy)]
enum StyleXScalarKind {
    String,
    Number,
    Other,
}

fn static_stylex_scalar_kind<'a>(
    expression: &'a Expression<'a>,
    before: u32,
    static_values: &FxHashMap<&'a str, (u32, &'a Expression<'a>)>,
    imports: &FxHashMap<&'a str, (Lib, &'a str)>,
    root_reference_spans: &FxHashSet<Span>,
    visiting: &mut FxHashSet<&'a str>,
) -> Option<StyleXScalarKind> {
    match unwrap_transparent_expression(expression) {
        Expression::StringLiteral(_) => Some(StyleXScalarKind::String),
        Expression::NumericLiteral(_) => Some(StyleXScalarKind::Number),
        Expression::Identifier(identifier) => {
            if !root_reference_spans.contains(&identifier.span) {
                return None;
            }
            let name = identifier.name.as_str();
            let &(declaration_start, value) = static_values.get(name)?;
            if declaration_start >= before || !visiting.insert(name) {
                return None;
            }
            let kind = static_stylex_scalar_kind(
                value,
                declaration_start,
                static_values,
                imports,
                root_reference_spans,
                visiting,
            );
            visiting.remove(name);
            kind
        }
        Expression::CallExpression(call)
            if is_static_stylex_pure_call(
                call,
                before,
                static_values,
                imports,
                root_reference_spans,
                visiting,
            ) =>
        {
            static_stylex_scalar_call_result(&call.callee)
        }
        _ => None,
    }
}

fn static_stylex_scalar_call_result(callee: &Expression<'_>) -> Option<StyleXScalarKind> {
    match unwrap_transparent_expression(callee) {
        Expression::Identifier(identifier) => match identifier.name.as_str() {
            "String" => Some(StyleXScalarKind::String),
            "Number" => Some(StyleXScalarKind::Number),
            _ => None,
        },
        Expression::StaticMemberExpression(member) => match member.property.name.as_str() {
            "at" | "charAt" | "concat" | "padEnd" | "padStart" | "replace" | "replaceAll"
            | "slice" | "substring" | "toExponential" | "toFixed" | "toLowerCase"
            | "toPrecision" | "toString" | "toUpperCase" | "trim" | "trimEnd" | "trimStart" => {
                Some(StyleXScalarKind::String)
            }
            "valueOf" => match unwrap_transparent_expression(&member.object) {
                Expression::StringLiteral(_) => Some(StyleXScalarKind::String),
                Expression::NumericLiteral(_) => Some(StyleXScalarKind::Number),
                _ => Some(StyleXScalarKind::Other),
            },
            method if is_static_math_method(method) => Some(StyleXScalarKind::Number),
            _ => Some(StyleXScalarKind::Other),
        },
        _ => None,
    }
}

fn is_static_string_method(method: &str) -> bool {
    matches!(
        method,
        "at" | "charAt"
            | "charCodeAt"
            | "codePointAt"
            | "concat"
            | "endsWith"
            | "includes"
            | "indexOf"
            | "lastIndexOf"
            | "padEnd"
            | "padStart"
            | "replace"
            | "replaceAll"
            | "search"
            | "slice"
            | "startsWith"
            | "substring"
            | "toLowerCase"
            | "toString"
            | "toUpperCase"
            | "trim"
            | "trimEnd"
            | "trimStart"
            | "valueOf"
    )
}

fn is_static_math_method(method: &str) -> bool {
    matches!(
        method,
        "abs"
            | "acos"
            | "acosh"
            | "asin"
            | "asinh"
            | "atan"
            | "atan2"
            | "atanh"
            | "cbrt"
            | "ceil"
            | "clz32"
            | "cos"
            | "cosh"
            | "exp"
            | "expm1"
            | "floor"
            | "fround"
            | "hypot"
            | "imul"
            | "log"
            | "log10"
            | "log1p"
            | "log2"
            | "max"
            | "min"
            | "pow"
            | "round"
            | "sign"
            | "sin"
            | "sinh"
            | "sqrt"
            | "tan"
            | "tanh"
            | "trunc"
    )
}

fn is_static_stylex_entries_call<'a>(
    call: &'a CallExpression<'a>,
    before: u32,
    static_values: &FxHashMap<&'a str, (u32, &'a Expression<'a>)>,
    imports: &FxHashMap<&'a str, (Lib, &'a str)>,
    root_reference_spans: &FxHashSet<Span>,
    visiting: &mut FxHashSet<&'a str>,
) -> bool {
    let Some(Expression::ArrayExpression(entries)) =
        call.arguments.first().and_then(Argument::as_expression)
    else {
        return false;
    };
    call.arguments.len() == 1
        && entries.elements.iter().all(|entry| {
            let Some(Expression::ArrayExpression(pair)) = entry.as_expression() else {
                return false;
            };
            let [key, value] = pair.elements.as_slice() else {
                return false;
            };
            let (Some(key), Some(value)) = (key.as_expression(), value.as_expression()) else {
                return false;
            };
            is_static_stylex_computed_key(
                key,
                before,
                static_values,
                imports,
                root_reference_spans,
                visiting,
            ) && is_static_stylex_theme_override(
                value,
                before,
                static_values,
                imports,
                root_reference_spans,
                visiting,
            )
        })
}

fn is_static_number_method(method: &str) -> bool {
    matches!(
        method,
        "toExponential" | "toFixed" | "toPrecision" | "toString" | "valueOf"
    )
}

fn is_static_stylex_scalar_call_shape<'a>(
    call: &'a CallExpression<'a>,
    before: u32,
    static_values: &FxHashMap<&'a str, (u32, &'a Expression<'a>)>,
    root_reference_spans: &FxHashSet<Span>,
) -> bool {
    let Expression::StaticMemberExpression(member) = unwrap_transparent_expression(&call.callee)
    else {
        return true;
    };
    match member.property.name.as_str() {
        "padStart" | "padEnd" => {
            static_padding_arguments_are_safe(call, before, static_values, root_reference_spans)
        }
        "at" | "charAt" | "charCodeAt" | "codePointAt" | "endsWith" | "includes" | "indexOf"
        | "lastIndexOf" | "slice" | "startsWith" | "substring" => call
            .arguments
            .iter()
            .filter_map(Argument::as_expression)
            .all(|argument| {
                !is_static_stylex_bigint_value(
                    argument,
                    before,
                    static_values,
                    root_reference_spans,
                    &mut FxHashSet::default(),
                )
            }),
        "toFixed" | "toExponential" => static_numeric_method_argument(
            call,
            0.0,
            100.0,
            before,
            static_values,
            root_reference_spans,
        ),
        "toPrecision" => static_numeric_method_argument(
            call,
            1.0,
            100.0,
            before,
            static_values,
            root_reference_spans,
        ),
        "toString"
            if static_stylex_scalar_call_result(&call.callee)
                .is_some_and(|kind| matches!(kind, StyleXScalarKind::String)) =>
        {
            static_numeric_method_argument(
                call,
                2.0,
                36.0,
                before,
                static_values,
                root_reference_spans,
            )
        }
        "valueOf" => call.arguments.is_empty(),
        _ => true,
    }
}

fn static_numeric_method_argument<'a>(
    call: &'a CallExpression<'a>,
    min: f64,
    max: f64,
    before: u32,
    static_values: &FxHashMap<&'a str, (u32, &'a Expression<'a>)>,
    root_reference_spans: &FxHashSet<Span>,
) -> bool {
    let Some(argument) = call.arguments.first() else {
        return true;
    };
    if call.arguments.len() != 1 {
        return false;
    }
    let Some(argument) = argument.as_expression() else {
        return false;
    };
    static_stylex_numeric_value(
        argument,
        before,
        static_values,
        root_reference_spans,
        &mut FxHashSet::default(),
    )
    .is_some_and(|value| value.fract() == 0.0 && (min..=max).contains(&value))
}

fn static_padding_arguments_are_safe<'a>(
    call: &'a CallExpression<'a>,
    before: u32,
    static_values: &FxHashMap<&'a str, (u32, &'a Expression<'a>)>,
    root_reference_spans: &FxHashSet<Span>,
) -> bool {
    const MAX_STATIC_PADDING: f64 = 1_000_000.0;
    let Some(target) = call.arguments.first() else {
        return true;
    };
    if call.arguments.len() > 2 {
        return false;
    }
    let Some(target) = target.as_expression() else {
        return false;
    };
    static_stylex_numeric_value(
        target,
        before,
        static_values,
        root_reference_spans,
        &mut FxHashSet::default(),
    )
    .is_some_and(|value| value.is_finite() && (0.0..=MAX_STATIC_PADDING).contains(&value))
}

fn static_stylex_numeric_value<'a>(
    expression: &'a Expression<'a>,
    before: u32,
    static_values: &FxHashMap<&'a str, (u32, &'a Expression<'a>)>,
    root_reference_spans: &FxHashSet<Span>,
    visiting: &mut FxHashSet<&'a str>,
) -> Option<f64> {
    match unwrap_transparent_expression(expression) {
        Expression::NumericLiteral(value) => Some(value.value),
        Expression::Identifier(identifier) => {
            if !root_reference_spans.contains(&identifier.span) {
                return None;
            }
            let name = identifier.name.as_str();
            let &(declaration_start, value) = static_values.get(name)?;
            if declaration_start >= before || !visiting.insert(name) {
                return None;
            }
            let number = static_stylex_numeric_value(
                value,
                declaration_start,
                static_values,
                root_reference_spans,
                visiting,
            );
            visiting.remove(name);
            number
        }
        _ => None,
    }
}

fn is_static_stylex_local_arrow_call<'a>(
    call: &'a CallExpression<'a>,
    before: u32,
    static_values: &FxHashMap<&'a str, (u32, &'a Expression<'a>)>,
    imports: &FxHashMap<&'a str, (Lib, &'a str)>,
    root_reference_spans: &FxHashSet<Span>,
    visiting: &mut FxHashSet<&'a str>,
) -> bool {
    let Expression::Identifier(callee) = unwrap_transparent_expression(&call.callee) else {
        return false;
    };
    if !root_reference_spans.contains(&callee.span) {
        return false;
    }
    let name = callee.name.as_str();
    let Some(&(declaration_start, value)) = static_values.get(name) else {
        return false;
    };
    let Expression::ArrowFunctionExpression(arrow) = unwrap_transparent_expression(value) else {
        return false;
    };
    if declaration_start >= before
        || arrow.r#async
        || arrow.params.rest.is_some()
        || arrow.params.items.len() != call.arguments.len()
        || !visiting.insert(name)
    {
        return false;
    }
    let mut parameters = FxHashMap::default();
    for (parameter, argument) in arrow.params.items.iter().zip(&call.arguments) {
        let (BindingPattern::BindingIdentifier(binding), Some(argument)) =
            (&parameter.pattern, argument.as_expression())
        else {
            visiting.remove(name);
            return false;
        };
        parameters.insert(binding.name.as_str(), argument);
    }
    let arguments_static = call.arguments.iter().all(|argument| {
        argument.as_expression().is_some_and(|argument| {
            is_static_stylex_theme_override(
                argument,
                before,
                static_values,
                imports,
                root_reference_spans,
                visiting,
            )
        })
    });
    let context = StyleXArrowStaticContext {
        before,
        static_values,
        imports,
        root_reference_spans,
        parameters,
    };
    let body_static = arguments_static
        && stylex_arrow_expression_body(arrow)
            .is_some_and(|body| is_static_stylex_arrow_body(body, &context, visiting));
    visiting.remove(name);
    body_static
}

struct StyleXArrowStaticContext<'maps, 'ast> {
    before: u32,
    static_values: &'maps FxHashMap<&'ast str, (u32, &'ast Expression<'ast>)>,
    imports: &'maps FxHashMap<&'ast str, (Lib, &'ast str)>,
    root_reference_spans: &'maps FxHashSet<Span>,
    parameters: FxHashMap<&'ast str, &'ast Expression<'ast>>,
}

fn stylex_arrow_expression_body<'a>(
    arrow: &'a ArrowFunctionExpression<'a>,
) -> Option<&'a Expression<'a>> {
    if !arrow.expression {
        return None;
    }
    match arrow.body.statements.first() {
        Some(Statement::ExpressionStatement(statement)) => Some(&statement.expression),
        _ => None,
    }
}

fn resolve_stylex_arrow_parameter_member<'a>(
    expression: &'a Expression<'a>,
    context: &StyleXArrowStaticContext<'_, 'a>,
    visiting: &mut FxHashSet<&'a str>,
) -> Option<ResolvedStyleXStaticMember<'a>> {
    let (base, _, segments) = binding_access_object_chain(expression)?;
    let argument = context.parameters.get(base)?;
    let mut resolver = StyleXStaticResolver {
        before: context.before,
        static_values: context.static_values,
        root_reference_spans: context.root_reference_spans,
    };
    let mut object = resolve_static_stylex_object(argument, &resolver, visiting)?;
    let mut segments = segments.into_iter().peekable();
    while let Some(segment) = segments.next() {
        let member = resolve_static_stylex_object_property(object, &segment, &resolver, visiting)?;
        if segments.peek().is_none() {
            return Some(member);
        }
        resolver = StyleXStaticResolver {
            before: member.before,
            ..resolver
        };
        object = resolve_static_stylex_object(member.value, &resolver, visiting)?;
    }
    None
}

fn is_static_stylex_arrow_body<'a>(
    expression: &'a Expression<'a>,
    context: &StyleXArrowStaticContext<'_, 'a>,
    visiting: &mut FxHashSet<&'a str>,
) -> bool {
    let expression = unwrap_transparent_expression(expression);
    match expression {
        Expression::Identifier(identifier)
            if context.parameters.contains_key(identifier.name.as_str()) =>
        {
            true
        }
        Expression::TemplateLiteral(template) => template
            .expressions
            .iter()
            .all(|expression| is_static_stylex_arrow_body(expression, context, visiting)),
        Expression::UnaryExpression(unary) => {
            let bigint_context = StyleXBigIntContext {
                before: context.before,
                static_values: context.static_values,
                root_reference_spans: context.root_reference_spans,
                parameters: &context.parameters,
            };
            !unary.operator.is_delete()
                && (unary.operator != UnaryOperator::UnaryPlus
                    || !is_static_stylex_bigint_value_with_context(
                        &unary.argument,
                        &bigint_context,
                        &mut visiting.clone(),
                    ))
                && is_static_stylex_arrow_body(&unary.argument, context, visiting)
        }
        Expression::BinaryExpression(binary) => {
            let bigint_context = StyleXBigIntContext {
                before: context.before,
                static_values: context.static_values,
                root_reference_spans: context.root_reference_spans,
                parameters: &context.parameters,
            };
            !binary.operator.is_relational()
                && (!binary.operator.is_numeric_or_string_binary_operator()
                    || !is_static_stylex_bigint_pair(
                        &binary.left,
                        &binary.right,
                        &bigint_context,
                        &mut visiting.clone(),
                    ))
                && is_static_stylex_arrow_body(&binary.left, context, visiting)
                && is_static_stylex_arrow_body(&binary.right, context, visiting)
        }
        Expression::LogicalExpression(logical) => {
            is_static_stylex_arrow_body(&logical.left, context, visiting)
                && is_static_stylex_arrow_body(&logical.right, context, visiting)
        }
        Expression::ConditionalExpression(conditional) => [
            &conditional.test,
            &conditional.consequent,
            &conditional.alternate,
        ]
        .into_iter()
        .all(|expression| is_static_stylex_arrow_body(expression, context, visiting)),
        Expression::SequenceExpression(sequence) => sequence
            .expressions
            .iter()
            .all(|expression| is_static_stylex_arrow_body(expression, context, visiting)),
        Expression::StaticMemberExpression(_) | Expression::ComputedMemberExpression(_) => {
            if let Some(resolved) =
                resolve_stylex_arrow_parameter_member(expression, context, visiting)
            {
                return is_static_stylex_theme_override(
                    resolved.value,
                    resolved.before,
                    context.static_values,
                    context.imports,
                    context.root_reference_spans,
                    visiting,
                );
            }
            is_static_stylex_theme_override(
                expression,
                context.before,
                context.static_values,
                context.imports,
                context.root_reference_spans,
                visiting,
            )
        }
        _ => is_static_stylex_theme_override(
            expression,
            context.before,
            context.static_values,
            context.imports,
            context.root_reference_spans,
            visiting,
        ),
    }
}

fn is_root_stylex_static_call(
    callee: &Expression<'_>,
    imports: &FxHashMap<&str, (Lib, &str)>,
    root_reference_spans: &FxHashSet<Span>,
) -> bool {
    match unwrap_transparent_expression(callee) {
        Expression::Identifier(identifier) => {
            root_reference_spans.contains(&identifier.span)
                && imports
                    .get(identifier.name.as_str())
                    .is_some_and(|(lib, role)| {
                        *lib == Lib::StyleX
                            && matches!(*role, "unstable_conditional" | "keyframes" | "positionTry")
                    })
        }
        Expression::StaticMemberExpression(member) => match &member.object {
            Expression::Identifier(object) => {
                root_reference_spans.contains(&object.span)
                    && imports
                        .get(object.name.as_str())
                        .is_some_and(|(lib, role)| {
                            *lib == Lib::StyleX
                                && (if *role == "types" {
                                    is_stylex_type_helper(member.property.name.as_str())
                                } else {
                                    matches!(
                                        member.property.name.as_str(),
                                        "unstable_conditional" | "keyframes" | "positionTry"
                                    )
                                })
                        })
            }
            Expression::StaticMemberExpression(namespace) => {
                let Expression::Identifier(object) = &namespace.object else {
                    return false;
                };
                root_reference_spans.contains(&object.span)
                    && imports
                        .get(object.name.as_str())
                        .is_some_and(|(lib, _)| *lib == Lib::StyleX)
                    && namespace.property.name.as_str() == "types"
                    && is_stylex_type_helper(member.property.name.as_str())
            }
            _ => false,
        },
        _ => false,
    }
}

fn is_stylex_type_helper(name: &str) -> bool {
    matches!(
        name,
        "angle"
            | "color"
            | "image"
            | "integer"
            | "length"
            | "lengthPercentage"
            | "number"
            | "percentage"
            | "resolution"
            | "time"
            | "transformFunction"
            | "transformList"
            | "url"
    )
}

/// The value of a static string or numeric computed-member key, or `None` for a
/// dynamic key that cannot be resolved without executing code.
fn static_computed_key(expr: &Expression<'_>) -> Option<String> {
    match unwrap_transparent_expression(expr) {
        Expression::StringLiteral(lit) => Some(lit.value.to_string()),
        Expression::NumericLiteral(lit) => Some(format_numeric_token(lit)),
        Expression::TemplateLiteral(template) if template.expressions.is_empty() => template
            .quasis
            .first()
            .map(|quasi| quasi.value.raw.to_string()),
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct RootBinding<'a> {
    name: &'a str,
    span: Span,
    reference_id: Option<ReferenceId>,
}

fn root_binding<'a>(identifier: &'a IdentifierReference<'_>) -> RootBinding<'a> {
    RootBinding {
        name: identifier.name.as_str(),
        span: identifier.span,
        reference_id: identifier.reference_id.get(),
    }
}

fn expression_root_binding<'a, 'b: 'a>(expr: &'a Expression<'b>) -> Option<RootBinding<'a>> {
    match expr {
        Expression::Identifier(id) => Some(root_binding(id)),
        Expression::StaticMemberExpression(member) => expression_root_binding(&member.object),
        Expression::ComputedMemberExpression(member) => expression_root_binding(&member.object),
        Expression::ParenthesizedExpression(expression) => {
            expression_root_binding(&expression.expression)
        }
        Expression::TSAsExpression(expression) => expression_root_binding(&expression.expression),
        Expression::TSSatisfiesExpression(expression) => {
            expression_root_binding(&expression.expression)
        }
        Expression::TSNonNullExpression(expression) => {
            expression_root_binding(&expression.expression)
        }
        Expression::TSTypeAssertion(expression) => expression_root_binding(&expression.expression),
        _ => None,
    }
}

fn assignment_target_root_bindings<'a, 'b: 'a>(
    target: &'a AssignmentTarget<'b>,
) -> Vec<RootBinding<'a>> {
    let mut bindings = Vec::new();
    collect_assignment_target_root_bindings(target, &mut bindings);
    bindings
}

fn assignment_target_receiver_expression<'a, 'b: 'a>(
    target: &'a AssignmentTarget<'b>,
) -> Option<&'a Expression<'b>> {
    match target {
        AssignmentTarget::StaticMemberExpression(member) => Some(&member.object),
        AssignmentTarget::ComputedMemberExpression(member) => Some(&member.object),
        AssignmentTarget::TSAsExpression(expression) => {
            call_receiver_expression(&expression.expression)
        }
        AssignmentTarget::TSSatisfiesExpression(expression) => {
            call_receiver_expression(&expression.expression)
        }
        AssignmentTarget::TSNonNullExpression(expression) => {
            call_receiver_expression(&expression.expression)
        }
        AssignmentTarget::TSTypeAssertion(expression) => {
            call_receiver_expression(&expression.expression)
        }
        _ => None,
    }
}

fn collect_assignment_target_root_bindings<'a, 'b: 'a>(
    target: &'a AssignmentTarget<'b>,
    bindings: &mut Vec<RootBinding<'a>>,
) {
    match target {
        AssignmentTarget::AssignmentTargetIdentifier(id) => bindings.push(root_binding(id)),
        AssignmentTarget::StaticMemberExpression(member) => {
            bindings.extend(expression_root_binding(&member.object));
        }
        AssignmentTarget::ComputedMemberExpression(member) => {
            bindings.extend(expression_root_binding(&member.object));
        }
        AssignmentTarget::TSAsExpression(expression) => {
            bindings.extend(expression_root_binding(&expression.expression));
        }
        AssignmentTarget::TSSatisfiesExpression(expression) => {
            bindings.extend(expression_root_binding(&expression.expression));
        }
        AssignmentTarget::TSNonNullExpression(expression) => {
            bindings.extend(expression_root_binding(&expression.expression));
        }
        AssignmentTarget::TSTypeAssertion(expression) => {
            bindings.extend(expression_root_binding(&expression.expression));
        }
        AssignmentTarget::ArrayAssignmentTarget(array) => {
            for element in array.elements.iter().flatten() {
                collect_assignment_target_maybe_default_root_bindings(element, bindings);
            }
            if let Some(rest) = &array.rest {
                collect_assignment_target_root_bindings(&rest.target, bindings);
            }
        }
        AssignmentTarget::ObjectAssignmentTarget(object) => {
            for property in &object.properties {
                match property {
                    AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(property) => {
                        bindings.push(root_binding(&property.binding));
                    }
                    AssignmentTargetProperty::AssignmentTargetPropertyProperty(property) => {
                        collect_assignment_target_maybe_default_root_bindings(
                            &property.binding,
                            bindings,
                        );
                    }
                }
            }
            if let Some(rest) = &object.rest {
                collect_assignment_target_root_bindings(&rest.target, bindings);
            }
        }
        AssignmentTarget::PrivateFieldExpression(_) => {}
    }
}

fn collect_assignment_target_maybe_default_root_bindings<'a, 'b: 'a>(
    target: &'a AssignmentTargetMaybeDefault<'b>,
    bindings: &mut Vec<RootBinding<'a>>,
) {
    if let AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(default) = target {
        collect_assignment_target_root_bindings(&default.binding, bindings);
    } else if let Some(target) = target.as_assignment_target() {
        collect_assignment_target_root_bindings(target, bindings);
    }
}

fn simple_assignment_target_root_binding<'a, 'b: 'a>(
    target: &'a SimpleAssignmentTarget<'b>,
) -> Option<RootBinding<'a>> {
    match target {
        SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => Some(root_binding(id)),
        SimpleAssignmentTarget::StaticMemberExpression(member) => {
            expression_root_binding(&member.object)
        }
        SimpleAssignmentTarget::ComputedMemberExpression(member) => {
            expression_root_binding(&member.object)
        }
        SimpleAssignmentTarget::TSAsExpression(expression) => {
            expression_root_binding(&expression.expression)
        }
        SimpleAssignmentTarget::TSSatisfiesExpression(expression) => {
            expression_root_binding(&expression.expression)
        }
        SimpleAssignmentTarget::TSNonNullExpression(expression) => {
            expression_root_binding(&expression.expression)
        }
        SimpleAssignmentTarget::TSTypeAssertion(expression) => {
            expression_root_binding(&expression.expression)
        }
        SimpleAssignmentTarget::PrivateFieldExpression(_) => None,
    }
}

/// Where the access binding comes from for a recognized token-definition call.
#[derive(Clone, Copy)]
enum BindingSource {
    /// The assigned identifier (`const vars = ...`).
    LhsIdent,
    /// An element of an array-destructure (`const [_, vars] = ...`).
    TupleElement(usize),
}

/// A recognized token-definition call: where the binding comes from and which
/// argument carries the token object.
#[derive(Clone, Copy)]
struct Recognized {
    binding_source: BindingSource,
    tokens_arg: usize,
    origin: CssInJsTokenOrigin,
    stylex_shape: Option<StyleXTokenShape>,
}

#[derive(Clone, Copy)]
enum StyleXTokenShape {
    Flat,
    Nested,
}

/// Collects token-definition sites, gated on import provenance.
struct TokenDefCollector<'a> {
    lines: LineCounter<'a>,
    /// local-binding name -> (library, canonical role). Mirrors the
    /// `css_in_js_object` provenance map but for token-definition roles.
    imports: FxHashMap<&'a str, (Lib, &'a str)>,
    /// Top-level immutable object literals available to macro calls in source
    /// order. Mutable or non-object bindings are deliberately absent.
    const_objects: FxHashMap<SymbolId, (u32, &'a ObjectExpression<'a>)>,
    /// Every resolved reference to a top-level immutable object literal.
    const_object_references: FxHashMap<ReferenceId, SymbolId>,
    /// Top-level constant condition names used by computed StyleX condition keys.
    const_strings: FxHashMap<ReferenceId, (u32, &'a str)>,
    nested_depth: u32,
    collecting_mutations: bool,
    defs: Vec<CssInJsTokenDef>,
}

impl<'a> TokenDefCollector<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            lines: LineCounter::new(source),
            imports: FxHashMap::default(),
            const_objects: FxHashMap::default(),
            const_object_references: FxHashMap::default(),
            const_strings: FxHashMap::default(),
            nested_depth: 0,
            collecting_mutations: false,
            defs: Vec::new(),
        }
    }

    /// Map each import binding from a recognized token library to its library +
    /// canonical role. Named imports dispatch on the imported (canonical) name so
    /// `import { createTheme as ct }` still fires; default / namespace bindings
    /// (`import * as stylex`) carry the local name for member-call recognition.
    fn build_import_map(&mut self, program: &Program<'a>) {
        for stmt in &program.body {
            let Statement::ImportDeclaration(decl) = stmt else {
                continue;
            };
            if decl.import_kind.is_type() {
                continue;
            }
            let Some(lib) = module_library(decl.source.value.as_str()) else {
                continue;
            };
            let Some(specifiers) = &decl.specifiers else {
                continue;
            };
            for specifier in specifiers {
                let (local, role) = match specifier {
                    ImportDeclarationSpecifier::ImportSpecifier(s) if !s.import_kind.is_type() => {
                        (s.local.name.as_str(), s.imported.name().as_str())
                    }
                    ImportDeclarationSpecifier::ImportSpecifier(_) => continue,
                    ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                        (s.local.name.as_str(), s.local.name.as_str())
                    }
                    ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                        (s.local.name.as_str(), s.local.name.as_str())
                    }
                };
                self.imports.insert(local, (lib, role));
            }
        }
    }

    fn build_const_object_map(&mut self, program: &'a Program<'a>, scoping: &Scoping) {
        for stmt in &program.body {
            let declaration = match stmt {
                Statement::VariableDeclaration(declaration) => Some(&**declaration),
                Statement::ExportNamedDeclaration(export) => match &export.declaration {
                    Some(Declaration::VariableDeclaration(declaration)) => Some(&**declaration),
                    _ => None,
                },
                _ => None,
            };
            let Some(declaration) = declaration else {
                continue;
            };
            if declaration.kind != VariableDeclarationKind::Const {
                continue;
            }
            for declarator in &declaration.declarations {
                let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
                    continue;
                };
                let Some(symbol_id) = binding.symbol_id.get() else {
                    continue;
                };
                let declaration_start = declarator.span.start;
                match declarator.init.as_ref().map(unwrap_transparent_expression) {
                    Some(Expression::ObjectExpression(obj)) => {
                        self.const_objects
                            .insert(symbol_id, (declaration_start, obj));
                        self.const_object_references.extend(
                            scoping
                                .get_resolved_reference_ids(symbol_id)
                                .iter()
                                .copied()
                                .map(|reference_id| (reference_id, symbol_id)),
                        );
                    }
                    Some(Expression::StringLiteral(value)) => {
                        self.const_strings.extend(
                            scoping
                                .get_resolved_reference_ids(symbol_id)
                                .iter()
                                .copied()
                                .map(|reference_id| {
                                    (reference_id, (declaration_start, value.value.as_str()))
                                }),
                        );
                    }
                    _ => {}
                }
            }
        }
    }

    /// Resolve a call's callee to `(library, role)` if its binding is a recognized
    /// token-library import. Handles both a named/aliased import callee
    /// (`defineVars(...)`) and a namespace member call (`stylex.defineVars(...)`).
    fn callee_role(&self, callee: &Expression<'a>) -> Option<(Lib, &'a str)> {
        match callee {
            Expression::Identifier(id) => self.imports.get(id.name.as_str()).copied(),
            Expression::StaticMemberExpression(member) => {
                let Expression::Identifier(obj) = &member.object else {
                    return None;
                };
                let (lib, _) = *self.imports.get(obj.name.as_str())?;
                // Member-call role is the accessed property (`stylex.defineVars`).
                Some((lib, member.property.name.as_str()))
            }
            _ => None,
        }
    }

    /// Dispatch `(library, role, arg_count)` to a recognized token-definition
    /// form, or `None` (unrecognized, or a contract-implementation form whose
    /// contract is the canonical definition).
    fn recognize(lib: Lib, role: &str, arg_count: usize) -> Option<Recognized> {
        let single = |tokens_arg, origin, stylex_shape| {
            Some(Recognized {
                binding_source: BindingSource::LhsIdent,
                tokens_arg,
                origin,
                stylex_shape,
            })
        };
        match (lib, role) {
            // `defineVars(obj)` / `createThemeContract(obj)`: binding = the assigned
            // identifier, token object = arg 0.
            (Lib::StyleX, "defineVars") if arg_count >= 1 => {
                single(0, CssInJsTokenOrigin::StyleX, Some(StyleXTokenShape::Flat))
            }
            (Lib::StyleX, "unstable_defineVarsNested") if arg_count >= 1 => single(
                0,
                CssInJsTokenOrigin::StyleX,
                Some(StyleXTokenShape::Nested),
            ),
            (Lib::VanillaExtract, "createThemeContract") if arg_count >= 1 => {
                single(0, CssInJsTokenOrigin::VanillaExtract, None)
            }
            // 1-arg createTheme returns [themeClass, vars]; tokens on the second
            // destructure element. The 2-arg (contract, tokens) form fills an
            // existing contract and is skipped (createThemeContract is canonical).
            (Lib::VanillaExtract, "createTheme") if arg_count == 1 => Some(Recognized {
                binding_source: BindingSource::TupleElement(1),
                tokens_arg: 0,
                origin: CssInJsTokenOrigin::VanillaExtract,
                stylex_shape: None,
            }),
            // 2-arg createGlobalTheme(selector, tokens) returns the vars object;
            // the 3-arg (selector, contract, tokens) form returns void (contract
            // canonical), so only the 2-arg form is a definition site here.
            (Lib::VanillaExtract, "createGlobalTheme") if arg_count == 2 => {
                single(1, CssInJsTokenOrigin::VanillaExtract, None)
            }
            (Lib::Panda, "defineTokens") if arg_count >= 1 => {
                single(0, CssInJsTokenOrigin::Panda, None)
            }
            _ => None,
        }
    }

    /// Extract the access binding name from a declarator's binding pattern for the
    /// recognized binding source.
    fn binding_name(decl: &VariableDeclarator<'a>, source: BindingSource) -> Option<&'a str> {
        match source {
            BindingSource::LhsIdent => match &decl.id {
                BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
                _ => None,
            },
            BindingSource::TupleElement(index) => {
                let BindingPattern::ArrayPattern(arr) = &decl.id else {
                    return None;
                };
                let element = arr.elements.get(index)?.as_ref()?;
                match element {
                    BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
                    _ => None,
                }
            }
        }
    }

    fn process_declarator(&mut self, decl: &VariableDeclarator<'a>) {
        let Some(Expression::CallExpression(call)) = &decl.init else {
            return;
        };
        if self.process_panda_config_call(call) {
            return;
        }
        let Some((lib, role)) = self.callee_role(&call.callee) else {
            return;
        };
        let Some(recognized) = Self::recognize(lib, role, call.arguments.len()) else {
            return;
        };
        let Some(binding) = Self::binding_name(decl, recognized.binding_source) else {
            return;
        };
        let Some(obj) = self.resolve_object_argument(call, recognized.tokens_arg) else {
            return;
        };
        let mut tokens = Vec::new();
        let complete = if let Some(shape) = recognized.stylex_shape {
            let context = StyleXTokenContext {
                imports: &self.imports,
                const_strings: &self.const_strings,
                before: call.span.start,
            };
            collect_stylex_token_object(&mut self.lines, obj, "", shape, &context, &mut tokens)
        } else {
            collect_token_leaves(&mut self.lines, obj, "", recognized.origin, &mut tokens);
            true
        };
        if !complete {
            return;
        }
        if tokens.is_empty() {
            return;
        }
        self.defs.push(CssInJsTokenDef {
            binding: binding.to_owned(),
            origin: recognized.origin,
            tokens,
        });
    }

    fn resolve_object_argument(
        &self,
        call: &'a oxc_ast::ast::CallExpression<'a>,
        index: usize,
    ) -> Option<&'a ObjectExpression<'a>> {
        let expr = call.arguments.get(index)?.as_expression()?;
        match unwrap_transparent_expression(expr) {
            Expression::ObjectExpression(obj) => Some(obj),
            Expression::Identifier(id) => {
                let reference_id = id.reference_id.get()?;
                let symbol_id = self.const_object_references.get(&reference_id)?;
                let (declaration_start, object) = self.const_objects.get(symbol_id)?;
                (*declaration_start < call.span.start).then_some(*object)
            }
            _ => None,
        }
    }

    fn invalidate_const_object(&mut self, binding: Option<RootBinding<'_>>) {
        let Some(reference_id) = binding.and_then(|binding| binding.reference_id) else {
            return;
        };
        let Some(symbol_id) = self.const_object_references.get(&reference_id) else {
            return;
        };
        self.const_objects.remove(symbol_id);
    }

    fn process_panda_config_call(&mut self, call: &oxc_ast::ast::CallExpression<'a>) -> bool {
        let Some((Lib::Panda, "defineConfig")) = self.callee_role(&call.callee) else {
            return false;
        };
        let Some(Argument::ObjectExpression(obj)) = call.arguments.first() else {
            return true;
        };
        let mut tokens = Vec::new();
        collect_panda_config_token_leaves(&mut self.lines, obj, &mut tokens);
        if !tokens.is_empty() {
            self.defs.push(CssInJsTokenDef {
                binding: PANDA_CONFIG_BINDING.to_string(),
                origin: CssInJsTokenOrigin::Panda,
                tokens,
            });
        }
        true
    }
}

struct StyleXTokenContext<'maps, 'ast> {
    imports: &'maps FxHashMap<&'ast str, (Lib, &'ast str)>,
    const_strings: &'maps FxHashMap<ReferenceId, (u32, &'ast str)>,
    before: u32,
}

fn collect_stylex_token_object(
    lines: &mut LineCounter<'_>,
    obj: &ObjectExpression<'_>,
    prefix: &str,
    shape: StyleXTokenShape,
    context: &StyleXTokenContext<'_, '_>,
    out: &mut Vec<CssInJsToken>,
) -> bool {
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            return false;
        };
        let Some(key) = prop.key.static_name() else {
            return false;
        };
        let path = if prefix.is_empty() {
            key.to_string()
        } else {
            format!("{prefix}.{key}")
        };
        let line = lines.line_at(prop.key.span().start);
        match shape {
            StyleXTokenShape::Flat => out.push(CssInJsToken {
                path,
                def_line: line,
                value: stylex_static_token_value(&prop.value, context.imports),
            }),
            StyleXTokenShape::Nested => match unwrap_transparent_expression(&prop.value) {
                Expression::ObjectExpression(nested)
                    if !is_stylex_conditional_object(
                        nested,
                        context.const_strings,
                        context.before,
                    ) =>
                {
                    if !collect_stylex_token_object(lines, nested, &path, shape, context, out) {
                        return false;
                    }
                }
                value => out.push(CssInJsToken {
                    path,
                    def_line: line,
                    value: stylex_static_token_value(value, context.imports),
                }),
            },
        }
    }
    true
}

fn is_stylex_conditional_object(
    obj: &ObjectExpression<'_>,
    const_strings: &FxHashMap<ReferenceId, (u32, &str)>,
    before: u32,
) -> bool {
    let mut has_default = false;
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            return false;
        };
        let key = if prop.computed {
            match prop.key.as_expression() {
                Some(Expression::Identifier(id)) => {
                    let Some(reference_id) = id.reference_id.get() else {
                        return false;
                    };
                    let Some((declaration_start, condition)) = const_strings.get(&reference_id)
                    else {
                        return false;
                    };
                    if *declaration_start >= before {
                        return false;
                    }
                    (*condition).to_string()
                }
                Some(expression) => {
                    let Some(condition) = static_computed_key(expression) else {
                        return false;
                    };
                    condition
                }
                None => return false,
            }
        } else {
            let Some(key) = prop.key.static_name() else {
                return false;
            };
            key.to_string()
        };
        if key == "default" {
            has_default = true;
        } else if !key.starts_with('@') {
            return false;
        }
    }
    has_default
}

fn stylex_static_token_value(
    value: &Expression<'_>,
    imports: &FxHashMap<&str, (Lib, &str)>,
) -> Option<String> {
    match unwrap_transparent_expression(value) {
        Expression::ObjectExpression(obj) => obj.properties.iter().find_map(|prop| {
            let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                return None;
            };
            (prop.key.static_name().as_deref() == Some("default"))
                .then(|| stylex_static_token_value(&prop.value, imports))
                .flatten()
        }),
        Expression::CallExpression(call) if is_stylex_static_value_call(&call.callee, imports) => {
            call.arguments
                .first()
                .and_then(Argument::as_expression)
                .and_then(|value| stylex_static_token_value(value, imports))
        }
        _ => static_token_value(value),
    }
}

fn is_stylex_static_value_call(
    callee: &Expression<'_>,
    imports: &FxHashMap<&str, (Lib, &str)>,
) -> bool {
    match callee {
        Expression::Identifier(id) => imports
            .get(id.name.as_str())
            .is_some_and(|(lib, role)| *lib == Lib::StyleX && *role == "unstable_conditional"),
        Expression::StaticMemberExpression(member) => match &member.object {
            Expression::Identifier(object) => {
                imports
                    .get(object.name.as_str())
                    .is_some_and(|(lib, role)| {
                        *lib == Lib::StyleX
                            && (*role == "types"
                                || member.property.name.as_str() == "unstable_conditional")
                    })
            }
            Expression::StaticMemberExpression(namespace) => {
                let Expression::Identifier(object) = &namespace.object else {
                    return false;
                };
                imports.get(object.name.as_str()).is_some_and(|(lib, _)| {
                    *lib == Lib::StyleX && namespace.property.name.as_str() == "types"
                })
            }
            _ => false,
        },
        _ => false,
    }
}

/// Flatten an object literal into dotted LEAF paths. An inline-object value
/// recurses (an intermediate token GROUP, not a token); a value-producing
/// expression (string / number / `null` contract leaf / call like
/// `px(2 * grid)` / template / member access like `colors.red['500']`) is a LEAF
/// token. A BARE IDENTIFIER value (`palette: tailwindPalette`) is SKIPPED: it
/// references something whose structure is invisible here, most often an imported
/// token GROUP (recording it as a leaf would invent a phantom token and wrongly
/// credit every `vars.palette.<x>` access to it). Spreads and computed keys are
/// skipped because they cannot be resolved statically.
fn collect_token_leaves(
    lines: &mut LineCounter<'_>,
    obj: &ObjectExpression<'_>,
    prefix: &str,
    origin: CssInJsTokenOrigin,
    out: &mut Vec<CssInJsToken>,
) {
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            continue;
        };
        let Some(key) = prop.key.static_name() else {
            continue;
        };
        let path = if prefix.is_empty() {
            key.to_string()
        } else {
            format!("{prefix}.{key}")
        };
        match &prop.value {
            Expression::ObjectExpression(nested)
                if origin == CssInJsTokenOrigin::Panda
                    && !prefix.is_empty()
                    && object_has_static_key(nested, "value") =>
            {
                out.push(CssInJsToken {
                    path,
                    def_line: lines.line_at(prop.key.span().start),
                    value: object_static_property_value(nested, "value"),
                });
            }
            Expression::ObjectExpression(nested) => {
                collect_token_leaves(lines, nested, &path, origin, out);
            }
            // A bare identifier is an unresolvable reference, usually an imported
            // token group; do not record it as a leaf.
            Expression::Identifier(_) => {}
            _ => out.push(CssInJsToken {
                value: static_token_value(&prop.value),
                path,
                def_line: lines.line_at(prop.key.span().start),
            }),
        }
    }
}

fn object_static_property_value(obj: &ObjectExpression<'_>, wanted: &str) -> Option<String> {
    obj.properties.iter().find_map(|prop| {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            return None;
        };
        (prop.key.static_name().as_deref() == Some(wanted))
            .then(|| static_token_value(&prop.value))
            .flatten()
    })
}

fn static_token_value(value: &Expression<'_>) -> Option<String> {
    match value {
        Expression::StringLiteral(lit) => {
            let text = lit.value.as_str().trim();
            (!text.is_empty()).then(|| text.to_string())
        }
        Expression::NumericLiteral(num) => Some(format_numeric_token(num)),
        Expression::UnaryExpression(unary) if unary.operator == UnaryOperator::UnaryNegation => {
            if let Expression::NumericLiteral(num) = &unary.argument {
                Some(format!("-{}", format_numeric_token(num)))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn format_numeric_token(num: &NumericLiteral<'_>) -> String {
    if num.value.fract() == 0.0 {
        format!("{:.0}", num.value)
    } else {
        num.value.to_string()
    }
}

fn is_theme_binding_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == "theme" || lower.ends_with("theme")
}

fn object_has_static_key(obj: &ObjectExpression<'_>, wanted: &str) -> bool {
    obj.properties.iter().any(|prop| {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            return false;
        };
        prop.key.static_name().is_some_and(|key| key == wanted)
    })
}

fn object_static_property_object<'a>(
    obj: &'a ObjectExpression<'a>,
    wanted: &str,
) -> Option<&'a ObjectExpression<'a>> {
    obj.properties.iter().find_map(|prop| {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            return None;
        };
        if prop.key.static_name().as_deref() == Some(wanted)
            && let Expression::ObjectExpression(value) = &prop.value
        {
            Some(&**value)
        } else {
            None
        }
    })
}

fn collect_panda_config_token_leaves(
    lines: &mut LineCounter<'_>,
    obj: &ObjectExpression<'_>,
    out: &mut Vec<CssInJsToken>,
) {
    let Some(theme) = object_static_property_object(obj, "theme") else {
        return;
    };
    for key in ["tokens", "semanticTokens"] {
        if let Some(tokens) = object_static_property_object(theme, key) {
            collect_token_leaves(lines, tokens, "", CssInJsTokenOrigin::Panda, out);
        }
    }
}

impl<'a> Visit<'a> for TokenDefCollector<'a> {
    fn visit_variable_declarator(&mut self, decl: &VariableDeclarator<'a>) {
        if !self.collecting_mutations && self.nested_depth == 0 {
            self.process_declarator(decl);
        }
        walk::walk_variable_declarator(self, decl);
    }

    fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
        self.nested_depth = self.nested_depth.saturating_add(1);
        walk::walk_function(self, function, flags);
        self.nested_depth = self.nested_depth.saturating_sub(1);
    }

    fn visit_arrow_function_expression(&mut self, function: &ArrowFunctionExpression<'a>) {
        self.nested_depth = self.nested_depth.saturating_add(1);
        walk::walk_arrow_function_expression(self, function);
        self.nested_depth = self.nested_depth.saturating_sub(1);
    }

    fn visit_block_statement(&mut self, block: &BlockStatement<'a>) {
        self.nested_depth = self.nested_depth.saturating_add(1);
        walk::walk_block_statement(self, block);
        self.nested_depth = self.nested_depth.saturating_sub(1);
    }

    fn visit_assignment_expression(&mut self, assignment: &AssignmentExpression<'a>) {
        if self.collecting_mutations {
            for binding in assignment_target_root_bindings(&assignment.left) {
                self.invalidate_const_object(Some(binding));
            }
        }
        walk::walk_assignment_expression(self, assignment);
    }

    fn visit_update_expression(&mut self, update: &UpdateExpression<'a>) {
        if self.collecting_mutations {
            self.invalidate_const_object(simple_assignment_target_root_binding(&update.argument));
        }
        walk::walk_update_expression(self, update);
    }

    fn visit_unary_expression(&mut self, expression: &UnaryExpression<'a>) {
        if self.collecting_mutations && expression.operator.is_delete() {
            self.invalidate_const_object(expression_root_binding(&expression.argument));
        }
        walk::walk_unary_expression(self, expression);
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if self.collecting_mutations && self.callee_role(&call.callee).is_none() {
            if let Some(receiver) = call_receiver_root(&call.callee) {
                self.invalidate_const_object(Some(receiver));
            }
            let possibly_mutated: Vec<RootBinding<'_>> = call
                .arguments
                .iter()
                .filter_map(Argument::as_expression)
                .filter_map(expression_root_binding)
                .collect();
            for binding in possibly_mutated {
                self.invalidate_const_object(Some(binding));
            }
        }
        walk::walk_call_expression(self, call);
    }

    fn visit_export_default_declaration(
        &mut self,
        decl: &oxc_ast::ast::ExportDefaultDeclaration<'a>,
    ) {
        if !self.collecting_mutations
            && let Some(Expression::CallExpression(call)) = decl.declaration.as_expression()
        {
            self.process_panda_config_call(call);
        }
        walk::walk_export_default_declaration(self, decl);
    }
}

/// Count `\n` bytes in `s` as a saturating `u32`.
fn count_newlines_u32(s: &str) -> u32 {
    u32::try_from(s.bytes().filter(|&b| b == b'\n').count()).unwrap_or(u32::MAX)
}

/// Incremental 1-based line-number counter over a fixed `source` (issue #1843
/// follow-up). The old free `line_at` counted the newlines in `source[..offset]`
/// from the start on every call, so a token file with M definitions cost
/// O(M * source-len). Definitions and consumer hits are visited in source order,
/// so this advances a cursor by only the newline delta since the previous query
/// (`source[last_offset..offset]`), making a whole walk O(source-len). A
/// non-monotonic query rewinds by the reverse delta, and an out-of-range or
/// non-char-boundary offset clamps to line 1 exactly as the previous `line_at`
/// did (matching `css::line_at_offset`), so the result is byte-identical to a
/// from-scratch count regardless of query order. Deliberately a plain cursor,
/// mirroring the `MAX_BINDING_PATH_DEPTH` bounded-work companions.
struct LineCounter<'a> {
    source: &'a str,
    /// Byte offset of the last query whose line was computed. Always a valid
    /// char boundary (only ever assigned a boundary-checked `end`).
    last_offset: usize,
    /// `1 + count_newlines(&source[..last_offset])`, the invariant maintained
    /// across queries.
    last_line: u32,
}

impl<'a> LineCounter<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            last_offset: 0,
            last_line: 1,
        }
    }

    /// 1-based line number of `offset`, byte-identical to the previous
    /// `line_at(source, offset)`.
    fn line_at(&mut self, offset: u32) -> u32 {
        let end = (offset as usize).min(self.source.len());
        // Preserve the previous `.get(..end)` contract: a non-char-boundary
        // offset clamps to line 1 rather than panicking on the slice below.
        if !self.source.is_char_boundary(end) {
            return 1;
        }
        if end >= self.last_offset {
            let delta = count_newlines_u32(&self.source[self.last_offset..end]);
            self.last_line = self.last_line.saturating_add(delta);
        } else {
            let delta = count_newlines_u32(&self.source[end..self.last_offset]);
            self.last_line = self.last_line.saturating_sub(delta);
        }
        self.last_offset = end;
        self.last_line
    }
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;

    fn defs(source: &str) -> Vec<CssInJsTokenDef> {
        css_in_js_token_defs(source, Path::new("tokens.ts"))
    }

    fn paths(defs: &[CssInJsTokenDef], binding: &str) -> Vec<String> {
        defs.iter()
            .find(|d| d.binding == binding)
            .map(|d| d.tokens.iter().map(|t| t.path.clone()).collect())
            .unwrap_or_default()
    }

    fn token_values(defs: &[CssInJsTokenDef], binding: &str) -> Vec<(String, Option<String>)> {
        defs.iter()
            .find(|d| d.binding == binding)
            .map(|d| {
                d.tokens
                    .iter()
                    .map(|t| (t.path.clone(), t.value.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn theme_defs(source: &str) -> Vec<CssInJsTokenDef> {
        css_in_js_theme_token_defs(source, Path::new("theme.ts"))
    }

    #[test]
    fn incremental_def_lines_match_source_order() {
        // Issue #1843 follow-up (FIX B): the incremental LineCounter must yield
        // the same def_line as a from-scratch newline count for every token,
        // across MULTIPLE definitions on multiple lines (the source-order cursor
        // must advance correctly between separate `defineVars` calls, not just
        // within one).
        let src = "import { defineVars } from '@stylexjs/stylex';\n\
export const colors = defineVars({\n\
primary: '#000',\n\
secondary: '#fff',\n\
});\n\
export const space = defineVars({\n\
sm: '4px',\n\
lg: '16px',\n\
});\n";
        let d = defs(src);
        let line_of = |binding: &str, path: &str| {
            d.iter()
                .find(|def| def.binding == binding)
                .and_then(|def| def.tokens.iter().find(|t| t.path == path))
                .unwrap_or_else(|| panic!("token {binding}.{path} present"))
                .def_line
        };
        assert_eq!(line_of("colors", "primary"), 3);
        assert_eq!(line_of("colors", "secondary"), 4);
        assert_eq!(line_of("space", "sm"), 7);
        assert_eq!(line_of("space", "lg"), 8);
    }

    #[test]
    fn stylex_define_vars_flat_namespace_call() {
        let d = defs(
            r"
import * as stylex from '@stylexjs/stylex';
export const vars = stylex.defineVars({ primaryColor: '#3b82f6', spacingSm: '4px' });
",
        );
        assert_eq!(paths(&d, "vars"), vec!["primaryColor", "spacingSm"]);
        assert_eq!(
            token_values(&d, "vars"),
            vec![
                ("primaryColor".to_string(), Some("#3b82f6".to_string())),
                ("spacingSm".to_string(), Some("4px".to_string())),
            ]
        );
    }

    #[test]
    fn stylex_define_vars_conditional_value_is_one_flat_token() {
        let d = defs(
            r"
import { defineVars } from '@stylexjs/stylex';
const DARK = '@media (prefers-color-scheme: dark)';
export const vars = defineVars({ color: { default: '#000', [DARK]: '#fff' } });
",
        );
        assert_eq!(paths(&d, "vars"), vec!["color"]);
        assert_eq!(
            token_values(&d, "vars"),
            vec![("color".to_string(), Some("#000".to_string()))]
        );
    }

    #[test]
    fn stylex_nested_vars_recurse_and_stop_at_conditional_leaves() {
        let d = defs(
            r"
import * as stylex from 'stylex';
import { unstable_conditional as cond } from 'stylex';
const DARK = '@media (prefers-color-scheme: dark)';
export const vars = stylex.unstable_defineVarsNested({
  surface: {
    bg: { default: '#fff', [DARK]: '#111' },
    text: cond({ default: '#000', [DARK]: '#eee' }),
  },
  typed: stylex.types.color('red'),
});
",
        );
        assert_eq!(
            paths(&d, "vars"),
            vec!["surface.bg", "surface.text", "typed"]
        );
        assert_eq!(
            token_values(&d, "vars"),
            vec![
                ("surface.bg".to_string(), Some("#fff".to_string())),
                ("surface.text".to_string(), Some("#000".to_string())),
                ("typed".to_string(), Some("red".to_string())),
            ]
        );
    }

    #[test]
    fn stylex_nested_default_token_namespace_is_not_a_conditional_leaf() {
        let d = defs(
            r"
import * as stylex from '@stylexjs/stylex';
export const vars = stylex.unstable_defineVarsNested({
  button: {
    primary: {
      background: {
        default: stylex.unstable_conditional({ default: 'blue' }),
        hovered: stylex.unstable_conditional({ default: 'navy' }),
      },
    },
  },
});
",
        );
        assert_eq!(
            paths(&d, "vars"),
            vec![
                "button.primary.background.default",
                "button.primary.background.hovered",
            ]
        );
    }

    #[test]
    fn stylex_nested_static_computed_conditions_remain_one_leaf() {
        let d = defs(
            r"
import * as stylex from '@stylexjs/stylex';
export const vars = stylex.unstable_defineVarsNested({
  surface: {
    color: {
      ['default']: '#fff',
      [`@media (prefers-color-scheme: dark)`]: '#111',
    },
  },
});
",
        );
        assert_eq!(paths(&d, "vars"), vec!["surface.color"]);
    }

    #[test]
    fn stylex_define_vars_resolves_local_const_object() {
        let d = defs(
            r"
import { defineVars as define } from '@stylexjs/stylex';
const values = { foreground: '#111', background: '#fff' };
export const vars = define(values);
",
        );
        assert_eq!(paths(&d, "vars"), vec!["foreground", "background"]);
    }

    #[test]
    fn stylex_define_vars_resolves_transparent_typescript_wrappers() {
        let d = defs(
            r"
import { defineVars } from '@stylexjs/stylex';
const values = { foreground: '#111' } as const;
export const vars = defineVars(values);
export const more = defineVars(
  { background: '#fff' } satisfies Record<string, string>,
);
",
        );
        assert_eq!(paths(&d, "vars"), vec!["foreground"]);
        assert_eq!(paths(&d, "more"), vec!["background"]);
    }

    #[test]
    fn stylex_define_vars_abstains_after_local_const_object_mutation() {
        let d = defs(
            r"
import { defineVars } from '@stylexjs/stylex';
const values = { foreground: '#111' };
values.foreground = getColor();
export const vars = defineVars(values);
",
        );
        assert!(d.is_empty(), "mutated token objects must abstain: {d:?}");
    }

    #[test]
    fn stylex_define_vars_abstains_when_local_object_is_mutated_after_definition() {
        let d = defs(
            r"
import { defineVars } from '@stylexjs/stylex';
const values = { foreground: '#111' };
export const vars = defineVars(values);
values.foreground = getColor();
",
        );
        assert!(
            d.is_empty(),
            "later mutation must invalidate the definition: {d:?}"
        );
    }

    #[test]
    fn stylex_define_vars_abstains_after_delete_or_receiver_call() {
        for mutation in ["delete values.foreground;", "values.mutate();"] {
            let source = format!(
                "import {{ defineVars }} from '@stylexjs/stylex';\nconst values = {{ foreground: '#111' }};\n{mutation}\nexport const vars = defineVars(values);"
            );
            let d = defs(&source);
            assert!(d.is_empty(), "mutated token objects must abstain: {source}");
        }
    }

    #[test]
    fn stylex_define_vars_requires_declared_unmutated_root_object() {
        for source in [
            r"
import { defineVars } from '@stylexjs/stylex';
export const vars = defineVars(values);
const values = { foreground: '#111' };
",
            r"
import { defineVars } from '@stylexjs/stylex';
const values = { foreground: '#111' };
({ foreground: values.foreground } = next);
export const vars = defineVars(values);
",
            r"
import { defineVars } from '@stylexjs/stylex';
const values = { foreground: '#111' };
[values.foreground] = next;
export const vars = defineVars(values);
",
        ] {
            let d = defs(source);
            assert!(d.is_empty(), "invalid static object must abstain: {source}");
        }
    }

    #[test]
    fn stylex_define_vars_ignores_shadowed_mutation() {
        let d = defs(
            r"
import { defineVars } from '@stylexjs/stylex';
const values = { foreground: '#111' };
function mutate(values) { values.foreground = '#fff'; }
export const vars = defineVars(values);
",
        );
        assert_eq!(paths(&d, "vars"), vec!["foreground"]);
    }

    #[test]
    fn stylex_nested_condition_must_be_declared_before_use() {
        let d = defs(
            r"
import { unstable_defineVarsNested } from '@stylexjs/stylex';
export const vars = unstable_defineVarsNested({
  surface: { color: { default: '#fff', [DARK]: '#111' } },
});
const DARK = '@media (prefers-color-scheme: dark)';
",
        );
        assert!(d.is_empty(), "TDZ condition must abstain: {d:?}");
    }

    #[test]
    fn stylex_nested_scope_shadow_does_not_define_tokens() {
        let d = defs(
            r"
import * as stylex from '@stylexjs/stylex';
export const makeVars = (stylex) => {
  const vars = stylex.defineVars({ foreground: '#111' });
  return vars;
};
",
        );
        assert!(d.is_empty(), "nested shadowed calls must abstain: {d:?}");
    }

    #[test]
    fn stylex_arbitrary_value_call_does_not_invent_comparable_value() {
        let d = defs(
            r"
import { defineVars } from '@stylexjs/stylex';
const dynamic = value => value;
export const vars = defineVars({ color: dynamic({ default: '#111' }) });
",
        );
        assert_eq!(paths(&d, "vars"), vec!["color"]);
        assert_eq!(token_values(&d, "vars"), vec![("color".to_string(), None)]);
    }

    #[test]
    fn panda_define_tokens_collapses_value_objects() {
        let d = defs(
            r"
import { defineTokens } from '@pandacss/dev';
export const tokens = defineTokens({
  colors: {
    brand: { value: '#f05a28' },
    accent: { value: '{colors.brand}' },
  },
  spacing: { card: { value: '1rem' } },
});
",
        );
        assert_eq!(
            paths(&d, "tokens"),
            vec!["colors.brand", "colors.accent", "spacing.card"]
        );
        assert_eq!(
            token_values(&d, "tokens"),
            vec![
                ("colors.brand".to_string(), Some("#f05a28".to_string())),
                (
                    "colors.accent".to_string(),
                    Some("{colors.brand}".to_string())
                ),
                ("spacing.card".to_string(), Some("1rem".to_string())),
            ]
        );
        assert_eq!(
            d.iter().find(|d| d.binding == "tokens").unwrap().origin,
            CssInJsTokenOrigin::Panda
        );
    }

    #[test]
    fn panda_define_config_extracts_tokens_and_semantic_tokens() {
        let d = defs(
            r"
import { defineConfig } from '@pandacss/dev';

export default defineConfig({
  theme: {
    tokens: {
      colors: {
        brand: { value: '#f05a28' },
      },
    },
    semanticTokens: {
      colors: {
        surface: { value: { base: '{colors.brand}', _dark: '#111111' } },
      },
    },
    recipes: {
      card: { base: { color: 'colors.brand' } },
    },
  },
});
",
        );
        assert_eq!(
            paths(&d, "pandaConfig"),
            vec!["colors.brand", "colors.surface"]
        );
        assert_eq!(
            token_values(&d, "pandaConfig"),
            vec![
                ("colors.brand".to_string(), Some("#f05a28".to_string())),
                ("colors.surface".to_string(), None),
            ]
        );
        assert_eq!(
            d.iter()
                .find(|d| d.binding == "pandaConfig")
                .unwrap()
                .origin,
            CssInJsTokenOrigin::Panda
        );
    }

    #[test]
    fn theme_object_definitions_flatten_static_leaves() {
        let d = theme_defs(
            r"
export const appTheme = {
  colors: { brand: '#f05a28', accent: '#111' },
  space: { card: '1rem' },
  dynamic: palette,
};
",
        );
        assert_eq!(
            paths(&d, "appTheme"),
            vec!["colors.brand", "colors.accent", "space.card"]
        );
        assert_eq!(
            token_values(&d, "appTheme"),
            vec![
                ("colors.brand".to_string(), Some("#f05a28".to_string())),
                ("colors.accent".to_string(), Some("#111".to_string())),
                ("space.card".to_string(), Some("1rem".to_string())),
            ]
        );
        assert_eq!(
            d.iter().find(|d| d.binding == "appTheme").unwrap().origin,
            CssInJsTokenOrigin::Theme
        );
    }

    #[test]
    fn theme_consumers_credit_props_and_destructured_theme_reads() {
        let leaves = ["colors.brand", "space.card"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let hits = scan_one(
            r"
import styled from 'styled-components';
export const Card = styled.div`
  color: ${({ theme }) => theme.colors.brand};
  margin: ${props => props.theme.space.card};
`;
",
            Path::new("card.tsx"),
            ConsumerQuery::ThemeReads {
                leaf_paths: &leaves,
            },
        );
        let mut token_paths: Vec<String> = hits.into_iter().map(|hit| hit.token_path).collect();
        token_paths.sort();
        assert_eq!(token_paths, vec!["colors.brand", "space.card"]);
    }

    #[test]
    fn ve_create_theme_tuple_destructure_binds_element_one() {
        let d = defs(
            r"
import { createTheme } from '@vanilla-extract/css';
export const [themeClass, vars] = createTheme({
  color: { brand: 'red', accent: 'blue' },
  space: { small: '4px' },
});
",
        );
        // Token paths bind to `vars` (element 1), NOT `themeClass`.
        assert_eq!(
            paths(&d, "vars"),
            vec!["color.brand", "color.accent", "space.small"]
        );
        assert!(paths(&d, "themeClass").is_empty());
    }

    #[test]
    fn ve_create_theme_contract_null_leaves() {
        let d = defs(
            r"
import { createThemeContract } from '@vanilla-extract/css';
export const vars = createThemeContract({ color: { brand: null, accent: null } });
",
        );
        // `null` contract leaves are tokens (the contract declares the shape).
        assert_eq!(paths(&d, "vars"), vec!["color.brand", "color.accent"]);
    }

    #[test]
    fn ve_create_global_theme_two_arg_binds_lhs_tokens_in_second_arg() {
        let d = defs(
            r"
import { createGlobalTheme } from '@vanilla-extract/css';
export const vars = createGlobalTheme(':root', { color: { brand: 'red' } });
",
        );
        assert_eq!(paths(&d, "vars"), vec!["color.brand"]);
    }

    #[test]
    fn ve_create_theme_two_arg_contract_impl_is_not_a_definition_site() {
        // The 2-arg form fills an existing contract (declared by
        // createThemeContract elsewhere); it must NOT introduce a binding.
        let d = defs(
            r"
import { createTheme } from '@vanilla-extract/css';
export const themeClass = createTheme(vars, { color: { brand: 'red' } });
",
        );
        assert!(
            d.is_empty(),
            "2-arg createTheme must not define tokens, got {d:?}"
        );
    }

    #[test]
    fn ve_create_global_theme_three_arg_contract_impl_is_not_a_definition_site() {
        let d = defs(
            r"
import { createGlobalTheme } from '@vanilla-extract/css';
createGlobalTheme(':root', vars, { color: { brand: 'red' } });
",
        );
        assert!(
            d.is_empty(),
            "3-arg createGlobalTheme must not define tokens, got {d:?}"
        );
    }

    #[test]
    fn aliased_named_import_still_fires() {
        let d = defs(
            r"
import { createThemeContract as ct } from '@vanilla-extract/css';
export const vars = ct({ color: { brand: null } });
",
        );
        assert_eq!(paths(&d, "vars"), vec!["color.brand"]);
    }

    #[test]
    fn local_helper_not_from_library_does_not_fire() {
        // A local `defineVars` shadowing the StyleX name must not be recognized.
        let d = defs(
            r"
function defineVars(o) { return o; }
export const vars = defineVars({ color: { primary: '#000' } });
",
        );
        assert!(d.is_empty(), "local defineVars must not fire, got {d:?}");
    }

    #[test]
    fn unrelated_create_theme_import_does_not_fire() {
        let d = defs(
            r"
import { createTheme } from '@mui/material/styles';
export const theme = createTheme({ palette: { primary: { main: '#000' } } });
",
        );
        assert!(d.is_empty(), "non-VE createTheme must not fire, got {d:?}");
    }

    #[test]
    fn type_only_import_does_not_fire() {
        let d = defs(
            r"
import type { defineVars } from '@stylexjs/stylex';
export const vars = defineVars({ color: { primary: '#000' } });
",
        );
        assert!(
            d.is_empty(),
            "type-only import must not gate recognition, got {d:?}"
        );
    }

    #[test]
    fn token_def_lines_are_per_leaf() {
        let src = "import { unstable_defineVarsNested } from '@stylexjs/stylex';\nexport const vars = unstable_defineVarsNested({\n  color: {\n    primary: '#000',\n    secondary: '#fff',\n  },\n});\n";
        let d = defs(src);
        let def = d.iter().find(|d| d.binding == "vars").unwrap();
        let primary = def
            .tokens
            .iter()
            .find(|t| t.path == "color.primary")
            .unwrap();
        let secondary = def
            .tokens
            .iter()
            .find(|t| t.path == "color.secondary")
            .unwrap();
        assert_eq!(primary.def_line, 4);
        assert_eq!(secondary.def_line, 5);
    }

    #[test]
    fn stylex_spread_or_dynamic_key_abstains_for_whole_definition() {
        let d = defs(
            r"
import { defineVars } from '@stylexjs/stylex';
const base = { a: '1' };
export const vars = defineVars({ ...base, ['x' + 'y']: '2', real: '#000' });
",
        );
        assert!(d.is_empty(), "partial StyleX shapes must abstain: {d:?}");
    }

    #[test]
    fn stylex_nested_dynamic_condition_key_abstains_for_whole_definition() {
        let d = defs(
            r"
import * as stylex from '@stylexjs/stylex';
export const vars = stylex.unstable_defineVarsNested({
  surface: { default: '#fff', [getCondition()]: '#111' },
});
",
        );
        assert!(
            d.is_empty(),
            "dynamic StyleX conditions must abstain: {d:?}"
        );
    }

    #[test]
    fn identifier_valued_key_is_not_a_leaf_but_call_and_member_values_are() {
        // `palette: tailwindPalette` (bare identifier, an imported group) must NOT
        // become a phantom `palette` leaf; `radius: px(2)` (call) and
        // `red: colors.red['500']` (member access) are real scalar leaves.
        let d = defs(
            r"
import { createGlobalTheme } from '@vanilla-extract/css';
export const vars = createGlobalTheme(':root', {
  palette: tailwindPalette,
  radius: px(2),
  red: colors.red['500'],
});
",
        );
        let p = paths(&d, "vars");
        assert!(
            !p.contains(&"palette".to_string()),
            "identifier-valued key must not be a leaf: {p:?}"
        );
        assert!(
            p.contains(&"radius".to_string()),
            "call-valued key is a leaf: {p:?}"
        );
        assert!(
            p.contains(&"red".to_string()),
            "member-valued key is a leaf: {p:?}"
        );
    }

    #[test]
    fn no_css_in_js_import_returns_empty() {
        let d = defs("export const vars = { color: { primary: '#000' } };");
        assert!(d.is_empty());
    }

    fn leaves(paths: &[&str]) -> FxHashSet<String> {
        paths.iter().map(|s| (*s).to_string()).collect()
    }

    /// Run one query and drop the query index from each hit.
    fn scan_one(source: &str, path: &Path, query: ConsumerQuery<'_>) -> Vec<TokenConsumerHit> {
        css_in_js_consumer_scan(source, path, &[query])
            .into_iter()
            .map(|(_, hit)| hit)
            .collect()
    }

    fn consumers(source: &str, alias: &str, paths: &[&str]) -> Vec<TokenConsumerHit> {
        let leaf_paths = leaves(paths);
        scan_one(
            source,
            Path::new("card.ts"),
            ConsumerQuery::MemberBinding {
                alias,
                leaf_paths: &leaf_paths,
            },
        )
    }

    fn panda_consumers(source: &str, alias: &str, paths: &[&str]) -> Vec<TokenConsumerHit> {
        let leaf_paths = leaves(paths);
        scan_one(
            source,
            Path::new("card.ts"),
            ConsumerQuery::PandaTokenCall {
                alias,
                leaf_paths: &leaf_paths,
            },
        )
    }

    fn panda_style_consumers(
        source: &str,
        aliases: &[&str],
        paths: &[&str],
    ) -> Vec<TokenConsumerHit> {
        let aliases = aliases.iter().map(|s| (*s).to_string()).collect();
        let leaf_paths = leaves(paths);
        scan_one(
            source,
            Path::new("card.ts"),
            ConsumerQuery::PandaStyleValues {
                aliases: &aliases,
                leaf_paths: &leaf_paths,
            },
        )
    }

    #[test]
    fn consumer_matches_deepest_leaf_not_intermediate_group() {
        // `vars.color.primary` is the leaf; `vars.color` (an intermediate group)
        // must NOT be counted, so exactly one hit per access site.
        let hits = consumers(
            "const a = vars.color.primary;",
            "vars",
            &["color.primary", "color.secondary"],
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].token_path, "color.primary");
        assert_eq!(hits[0].line, 1);
    }

    #[test]
    fn consumer_aliased_receiver() {
        // The caller passes the local alias; member access on it is matched.
        let hits = consumers("const a = v.color.primary;", "v", &["color.primary"]);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].token_path, "color.primary");
    }

    #[test]
    fn consumer_multiple_sites_distinct_lines() {
        let src = "const a = vars.color.primary;\nconst b = vars.space.sm;\nconst c = vars.color.primary;";
        let hits = consumers(src, "vars", &["color.primary", "space.sm"]);
        assert_eq!(hits.len(), 3);
        let lines: Vec<u32> = hits.iter().map(|h| h.line).collect();
        assert_eq!(lines, vec![1, 2, 3]);
    }

    #[test]
    fn consumer_in_style_object_value_position() {
        // The dominant real shape: a token read inside a style-call object value.
        let hits = consumers(
            "export const s = stylex.create({ root: { color: vars.color.primary } });",
            "vars",
            &["color.primary"],
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].token_path, "color.primary");
    }

    #[test]
    fn panda_token_call_consumer_matches_string_literal() {
        let hits = panda_consumers(
            "export const c = css({ color: token('colors.brand') });",
            "token",
            &["colors.brand", "colors.accent"],
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].token_path, "colors.brand");
    }

    #[test]
    fn panda_style_value_consumer_matches_known_token_string() {
        let hits = panda_style_consumers(
            "export const c = css({ color: 'colors.brand', _hover: { bg: 'colors.accent' } });",
            &["css"],
            &["colors.brand", "colors.accent", "colors.unused"],
        );
        let paths: Vec<_> = hits.iter().map(|hit| hit.token_path.as_str()).collect();
        assert_eq!(paths, vec!["colors.brand", "colors.accent"]);
    }

    #[test]
    fn panda_style_value_consumer_ignores_unimported_alias() {
        let hits = panda_style_consumers(
            "export const c = notPanda({ color: 'colors.brand' });",
            &["css"],
            &["colors.brand"],
        );
        assert!(hits.is_empty());
    }

    #[test]
    fn consumer_flat_stylex_depth_one() {
        let hits = consumers("const a = vars.primaryColor;", "vars", &["primaryColor"]);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].token_path, "primaryColor");
    }

    #[test]
    fn stylex_theme_call_consumes_complete_group_once() {
        let leaf_paths = leaves(&["surface.bg", "surface.text"]);
        let queries = [ConsumerQuery::StyleXThemeGroup {
            contract_alias: "tokens",
            leaf_paths: &leaf_paths,
        }];
        let hits = css_in_js_consumer_scan(
            "import { createTheme as theme } from 'stylex';\nconst reset = theme(tokens, {});",
            Path::new("theme.ts"),
            &queries,
        );
        let mut paths: Vec<_> = hits
            .into_iter()
            .map(|(_, hit)| (hit.token_path, hit.line))
            .collect();
        paths.sort();
        assert_eq!(
            paths,
            vec![
                ("surface.bg".to_string(), 2),
                ("surface.text".to_string(), 2),
            ]
        );
    }

    #[test]
    fn stylex_theme_call_abstains_for_type_only_or_unknown_contract() {
        let leaf_paths = leaves(&["surface.bg"]);
        let queries = [ConsumerQuery::StyleXThemeGroup {
            contract_alias: "tokens",
            leaf_paths: &leaf_paths,
        }];
        for source in [
            "import { type createTheme } from '@stylexjs/stylex'; createTheme(tokens, {});",
            "import * as stylex from '@stylexjs/stylex'; stylex.createTheme(other, {});",
            "const createTheme = (a, b) => b; createTheme(tokens, {});",
            "import { createTheme } from 'stylex'; const run = (createTheme) => createTheme(tokens, {});",
            "import * as stylex from 'stylex'; const run = (stylex) => stylex.createTheme(tokens, {});",
            "import { createTheme } from 'stylex'; const run = (tokens) => createTheme(tokens, {});",
        ] {
            assert!(
                css_in_js_consumer_scan(source, Path::new("theme.ts"), &queries).is_empty(),
                "must abstain: {source}"
            );
        }
    }

    #[test]
    fn stylex_theme_call_requires_bound_exact_static_shape() {
        let leaf_paths = leaves(&["surface.bg"]);
        let queries = [ConsumerQuery::StyleXThemeGroup {
            contract_alias: "tokens",
            leaf_paths: &leaf_paths,
        }];
        for source in [
            "import { createTheme } from 'stylex'; createTheme(tokens, {});",
            "import { createTheme } from 'stylex'; const theme = createTheme(tokens);",
            "import { createTheme } from 'stylex'; const theme = createTheme(tokens, {}, {});",
            "import { createTheme } from 'stylex'; const theme = createTheme(tokens, '#fff');",
            "import { createTheme } from 'stylex'; const theme = createTheme(tokens, getOverrides());",
            "import { createTheme } from 'stylex'; const { theme } = createTheme(tokens, {});",
        ] {
            assert!(
                css_in_js_consumer_scan(source, Path::new("theme.ts"), &queries).is_empty(),
                "must abstain: {source}"
            );
        }
    }

    #[test]
    fn stylex_theme_call_accepts_unmutated_local_static_override_and_wrapped_contract() {
        let leaf_paths = leaves(&["surface.bg"]);
        let queries = [ConsumerQuery::StyleXThemeGroup {
            contract_alias: "tokens",
            leaf_paths: &leaf_paths,
        }];
        let source = r"
import { createTheme, unstable_conditional } from 'stylex';
const overrides = {
  surface: unstable_conditional({ default: '#fff', '@media (prefers-color-scheme: dark)': '#111' }),
} as const;
const theme = createTheme(tokens as typeof tokens, overrides);
";
        let hits = css_in_js_consumer_scan(source, Path::new("theme.ts"), &queries);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].1.token_path, "surface.bg");
    }

    #[test]
    fn stylex_theme_call_accepts_official_static_expression_shapes() {
        let leaf_paths = leaves(&["surface.bg"]);
        let queries = [ConsumerQuery::StyleXThemeGroup {
            contract_alias: "tokens",
            leaf_paths: &leaf_paths,
        }];
        let source = r"
import { createTheme, types } from 'stylex';
const DARK = '@media (prefers-color-scheme: dark)';
const name = 'light';
const RADIUS = 4;
const theme = createTheme(tokens, {
  [DARK]: `${name}green`,
  radius: RADIUS * 2,
  typed: types.length({ default: RADIUS * 2 }),
});
";
        let hits = css_in_js_consumer_scan(source, Path::new("theme.ts"), &queries);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].1.token_path, "surface.bg");
    }

    #[test]
    fn stylex_theme_call_accepts_generic_evaluator_shapes() {
        let leaf_paths = leaves(&["surface.bg"]);
        let queries = [ConsumerQuery::StyleXThemeGroup {
            contract_alias: "tokens",
            leaf_paths: &leaf_paths,
        }];
        let source = r"
import { createTheme } from 'stylex';
const palette = { green: 'green' };
const base = { color: 'red' };
const alias = base;
const FLAG = true;
const RADIUS = '4';
const choose = value => value ? 'red' : 'blue';
const pick = value => value.color;
consume(RADIUS);
const theme = createTheme(tokens, {
  ...alias,
  member: palette.green,
  conditional: FLAG ? 'red' : 'blue',
  logical: FLAG && 'red',
  sequence: (0, 'red'),
  raw: String.raw`red-${2}`,
  math: Math.max(1, 2),
  stringMethod: 'red'.toUpperCase(),
  entries: Object.fromEntries([['color', 'red']]),
  arrow: choose(FLAG),
  arrowMember: pick({ color: 'red' }),
  coercedRadius: +RADIUS,
  bigintEquality: (1n === 1n) ? 8 : 4,
});
";
        let hits = css_in_js_consumer_scan(source, Path::new("theme.ts"), &queries);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].1.token_path, "surface.bg");
    }

    #[test]
    fn stylex_theme_call_resolves_static_contract_aliases_and_scalar_receivers() {
        let leaf_paths = leaves(&["surface.bg"]);
        let queries = [ConsumerQuery::StyleXThemeGroup {
            contract_alias: "tokens",
            leaf_paths: &leaf_paths,
        }];
        let source = r"
import { createTheme } from 'stylex';
const alias = tokens;
const secondAlias = alias;
const color = 'red';
const digits = 2;
const theme = createTheme(secondAlias, {
  color: color.toUpperCase().toLowerCase(),
  radius: (1).toFixed(digits),
});
color.toUpperCase();
const second = createTheme(alias, { color });
";
        let hits = css_in_js_consumer_scan(source, Path::new("theme.ts"), &queries);
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn stylex_pure_call_arguments_remain_static_across_theme_calls() {
        let leaf_paths = leaves(&["surface.bg"]);
        let queries = [ConsumerQuery::StyleXThemeGroup {
            contract_alias: "tokens",
            leaf_paths: &leaf_paths,
        }];
        let source = r"
import { createTheme } from 'stylex';
const RADIUS = 4;
const first = createTheme(tokens, { radius: Math.max(RADIUS, 2) });
const second = createTheme(tokens, { radius: RADIUS * 2 });
";
        let hits = css_in_js_consumer_scan(source, Path::new("theme.ts"), &queries);
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn stylex_theme_call_rejects_transitive_tdz_cycles_and_dynamic_helpers() {
        let leaf_paths = leaves(&["surface.bg"]);
        let queries = [ConsumerQuery::StyleXThemeGroup {
            contract_alias: "tokens",
            leaf_paths: &leaf_paths,
        }];
        for source in [
            "import { createTheme } from 'stylex'; const palette = { green: later }; const later = 'red'; const theme = createTheme(tokens, { color: palette.green });",
            "import { createTheme } from 'stylex'; const palette = { green: palette.green }; const theme = createTheme(tokens, { color: palette.green });",
            "import { createTheme } from 'stylex'; const choose = value => getColor(value); const theme = createTheme(tokens, { color: choose(true) });",
            "import { createTheme } from 'stylex'; const theme = createTheme(tokens, { color: Object.assign({}, { color: 'red' }) });",
            "import { createTheme } from 'stylex'; const theme = createTheme(tokens, { color: Object.fromEntries() });",
            "import { createTheme } from 'stylex'; const theme = createTheme(tokens, { color: Object.fromEntries([['color', ['red']]]) });",
            "import { createTheme } from 'stylex'; const theme = createTheme(tokens, { color: Math.notAFunction(1) });",
            "import { createTheme } from 'stylex'; const theme = createTheme(tokens, { color: 'x'.repeat(-1) });",
            "import { createTheme } from 'stylex'; const theme = createTheme(tokens, { color: (1).toFixed(1000) });",
            "import { createTheme } from 'stylex'; const theme = createTheme(tokens, { radius: 1n + 1 });",
            "import { createTheme } from 'stylex'; const theme = createTheme(tokens, { radius: +1n });",
            "import { createTheme } from 'stylex'; const theme = createTheme(tokens, { radius: Math.max(1n, 2n) });",
            "import { createTheme } from 'stylex'; const theme = createTheme(tokens, { color: 'x' in 1 });",
            "import { createTheme } from 'stylex'; const base = { color: 'red' }; const alias = base; alias.color = getColor(); const theme = createTheme(tokens, base);",
            "import { createTheme } from 'stylex'; const big = () => 1n; const theme = createTheme(tokens, { radius: Math.max(big(), 2) });",
            "import { createTheme } from 'stylex'; const big = () => LATER; const LATER = 1n; const theme = createTheme(tokens, { radius: Math.max(big(), 2) });",
            "import { createTheme } from 'stylex'; const base = { color: 'red' }; const get = () => base; get().color = getColor(); const theme = createTheme(tokens, base);",
            "import { createTheme } from 'stylex'; const base = { color: 'red' }; const get = () => base; mutate(get()); const theme = createTheme(tokens, base);",
            "import { createTheme } from 'stylex'; const base = { nested: { color: 'red' } }; const member = value => value.nested; member(base).color = getColor(); const theme = createTheme(tokens, base);",
            "import { createTheme } from 'stylex'; const base = { nested: { color: 'red' } }; const member = value => value.nested; mutate(member(base)); const theme = createTheme(tokens, base);",
            "import { createTheme } from 'stylex'; const base = { color: 'red' }; const theme = createTheme(tokens, base); base.color = getColor();",
            "import { createTheme } from 'stylex'; const choose = value => { return value; }; const theme = createTheme(tokens, { color: choose('red') });",
            "import { createTheme } from 'stylex'; const choose = value => { return value; mutate(); }; const theme = createTheme(tokens, { color: choose('red') });",
            "import { createTheme } from 'stylex'; const theme = createTheme(tokens, { color: 'a'.localeCompare('b', 'not_a_locale') });",
            "import { createTheme } from 'stylex'; const theme = createTheme(tokens, { color: 'x'.padStart(1 / 0, 'a') });",
            "import { createTheme } from 'stylex'; const theme = createTheme(tokens, { color: 'abc'.charAt(1n) });",
            "import { createTheme } from 'stylex'; const theme = createTheme(tokens, { color: 'abc'.includes('a', 1n) });",
            "import { createTheme } from 'stylex'; const theme = createTheme(tokens, { color: String(getFlag() ? 'a' : 'b') });",
            "import { createTheme } from 'stylex'; const theme = createTheme(tokens, { color: String((mutate(), 'red')) });",
            "import { createTheme } from 'stylex'; const theme = createTheme(tokens, { color: String(+1n) });",
            "import { createTheme } from 'stylex'; const theme = createTheme(tokens, { color: 'x'.concat(1n + 1n) });",
            "import { createTheme } from 'stylex'; const theme = createTheme(tokens, { color: String('x' in 1) });",
            "import { createTheme } from 'stylex'; const theme = createTheme(tokens, { color: Math.max(+1n, 2) });",
            "import { createTheme } from 'stylex'; const theme = createTheme(tokens, { [1n + 1]: 'red' });",
            "import { createTheme } from 'stylex'; const theme = createTheme(tokens, { [1n / 0n]: 'red' });",
            "import { createTheme } from 'stylex'; const alias = tokens; alias = other; const theme = createTheme(alias, { color: 'red' });",
        ] {
            assert!(
                css_in_js_consumer_scan(source, Path::new("theme.ts"), &queries).is_empty(),
                "must abstain: {source}"
            );
        }
    }

    #[test]
    fn stylex_theme_call_rejects_noncompiler_static_shapes() {
        let leaf_paths = leaves(&["surface.bg"]);
        let queries = [ConsumerQuery::StyleXThemeGroup {
            contract_alias: "tokens",
            leaf_paths: &leaf_paths,
        }];
        for source in [
            "import { createTheme } from 'stylex'; const theme = createTheme(tokens, { value: [] });",
            "import stylex, { createTheme } from 'stylex'; const theme = createTheme(tokens, { value: stylex.types.notAType({}) });",
            "import stylex, { createTheme } from 'stylex'; const theme = createTheme(tokens, { value: stylex.keyframes() });",
            "import { createTheme } from 'stylex'; const overrides = {}; const run = (overrides) => { const theme = createTheme(tokens, overrides); };",
            "import { createTheme } from 'stylex'; const overrides = {}; ({ value: overrides.value } = next); const theme = createTheme(tokens, overrides);",
        ] {
            assert!(
                css_in_js_consumer_scan(source, Path::new("theme.ts"), &queries).is_empty(),
                "must abstain: {source}"
            );
        }
    }

    #[test]
    fn consumer_other_binding_not_matched() {
        // A same-named member access on a DIFFERENT binding must not be a hit.
        let hits = consumers("const a = other.color.primary;", "vars", &["color.primary"]);
        assert!(hits.is_empty());
    }

    #[test]
    fn consumer_shadowed_token_alias_is_not_matched() {
        let hits = consumers(
            "const read = (vars) => vars.color.primary;",
            "vars",
            &["color.primary"],
        );
        assert!(hits.is_empty());
    }

    #[test]
    fn consumer_lexical_shadowing_respects_loops_catch_and_tdz() {
        let source = r"
import { vars } from './tokens.stylex';
for (const vars of groups) { use(vars.color.primary); }
try { work(); } catch (vars) { use(vars.color.primary); }
{ use(vars.color.primary); const vars = fallback; }
use((vars as typeof vars).color.primary);
";
        let hits = consumers(source, "vars", &["color.primary"]);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, 6);
    }

    #[test]
    fn stylex_theme_callee_loop_shadow_does_not_escape_loop_scope() {
        let leaf_paths = leaves(&["surface.bg"]);
        let queries = [ConsumerQuery::StyleXThemeGroup {
            contract_alias: "tokens",
            leaf_paths: &leaf_paths,
        }];
        let source = r"
import { createTheme } from 'stylex';
for (const createTheme of factories) {
  const bad = createTheme(tokens, {});
}
const good = createTheme(tokens, {});
";
        let hits = css_in_js_consumer_scan(source, Path::new("theme.ts"), &queries);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].1.line, 6);
    }

    #[test]
    fn consumer_deeper_access_past_leaf_matches_leaf_subexpression_once() {
        // `vars.color.primary.toString()` reads the leaf `color.primary`; the outer
        // `.toString` chain is not a leaf, the inner `vars.color.primary` is.
        let hits = consumers(
            "const a = vars.color.primary.toString();",
            "vars",
            &["color.primary"],
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].token_path, "color.primary");
    }

    #[test]
    fn consumer_undefined_path_not_matched() {
        let hits = consumers("const a = vars.color.tertiary;", "vars", &["color.primary"]);
        assert!(hits.is_empty());
    }

    #[test]
    fn consumer_bracket_notation_hyphenated_key() {
        // Hyphenated / digit-leading token keys are not valid JS identifiers, so
        // they are consumed via bracket notation; the leaf path keeps the raw key.
        let hits = consumers(
            "const a = vars.color['gray-100'];\nconst b = vars.borderRadius['0x'];",
            "vars",
            &["color.gray-100", "borderRadius.0x"],
        );
        let paths: Vec<&str> = hits.iter().map(|h| h.token_path.as_str()).collect();
        assert!(paths.contains(&"color.gray-100"));
        assert!(paths.contains(&"borderRadius.0x"));
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn consumer_mixed_dot_and_bracket_chain() {
        // `vars['color'].primary` and `vars.color['primary']` both reconstruct the
        // same `color.primary` leaf.
        let hits = consumers(
            "const a = vars['color'].primary;\nconst b = vars.color['primary'];",
            "vars",
            &["color.primary"],
        );
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().all(|h| h.token_path == "color.primary"));
    }

    #[test]
    fn consumer_numeric_computed_key_is_matched() {
        let hits = consumers("const a = vars.color.gray[50];", "vars", &["color.gray.50"]);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].token_path, "color.gray.50");
    }

    #[test]
    fn consumer_non_literal_computed_key_not_matched() {
        // A dynamic computed key cannot be resolved statically (lower-bound miss).
        let hits = consumers(
            "const k = 'primary'; const a = vars.color[k];",
            "vars",
            &["color.primary"],
        );
        assert!(hits.is_empty());
    }

    #[test]
    fn consumer_empty_inputs_short_circuit() {
        assert!(consumers("const a = vars.color.primary;", "", &["color.primary"]).is_empty());
        assert!(consumers("const a = vars.color.primary;", "vars", &[]).is_empty());
    }

    #[test]
    fn consumer_scan_matches_individual_calls() {
        // One source exercising all four query kinds; the scan must return exactly
        // the union of the four individual functions' hits, each tagged with the
        // index of the query that produced it.
        let source = "const a = vars.color.primary;\nconst b = css({ color: token('colors.brand'), background: 'colors.accent' });\nconst c = theme.space.card;";
        let path = Path::new("card.tsx");

        let member_leaves = leaves(&["color.primary"]);
        let panda_call_leaves = leaves(&["colors.brand"]);
        let panda_style_aliases = leaves(&["css"]);
        let panda_style_leaves = leaves(&["colors.accent"]);
        let theme_leaves = leaves(&["space.card"]);

        let queries = [
            ConsumerQuery::MemberBinding {
                alias: "vars",
                leaf_paths: &member_leaves,
            },
            ConsumerQuery::PandaTokenCall {
                alias: "token",
                leaf_paths: &panda_call_leaves,
            },
            ConsumerQuery::PandaStyleValues {
                aliases: &panda_style_aliases,
                leaf_paths: &panda_style_leaves,
            },
            ConsumerQuery::ThemeReads {
                leaf_paths: &theme_leaves,
            },
        ];
        let scanned = css_in_js_consumer_scan(source, path, &queries);

        let individual: Vec<(usize, TokenConsumerHit)> = scan_one(
            source,
            path,
            ConsumerQuery::MemberBinding {
                alias: "vars",
                leaf_paths: &member_leaves,
            },
        )
        .into_iter()
        .map(|hit| (0, hit))
        .chain(
            scan_one(
                source,
                path,
                ConsumerQuery::PandaTokenCall {
                    alias: "token",
                    leaf_paths: &panda_call_leaves,
                },
            )
            .into_iter()
            .map(|hit| (1, hit)),
        )
        .chain(
            scan_one(
                source,
                path,
                ConsumerQuery::PandaStyleValues {
                    aliases: &panda_style_aliases,
                    leaf_paths: &panda_style_leaves,
                },
            )
            .into_iter()
            .map(|hit| (2, hit)),
        )
        .chain(
            scan_one(
                source,
                path,
                ConsumerQuery::ThemeReads {
                    leaf_paths: &theme_leaves,
                },
            )
            .into_iter()
            .map(|hit| (3, hit)),
        )
        .collect();

        assert_eq!(scanned, individual);
        assert_eq!(scanned.len(), 4);
        assert_eq!(
            scanned[0],
            (
                0,
                TokenConsumerHit {
                    token_path: "color.primary".to_string(),
                    line: 1,
                }
            )
        );
        assert_eq!(
            scanned[3],
            (
                3,
                TokenConsumerHit {
                    token_path: "space.card".to_string(),
                    line: 3,
                }
            )
        );
    }

    #[test]
    fn consumer_scan_empty_query_is_isolated() {
        // An empty-alias query short-circuits to no hits WITHOUT suppressing the
        // valid query that follows it.
        let source = "const a = vars.color.primary;";
        let path = Path::new("card.ts");
        let empty_leaves = leaves(&["color.primary"]);
        let valid_leaves = leaves(&["color.primary"]);
        let queries = [
            ConsumerQuery::MemberBinding {
                alias: "",
                leaf_paths: &empty_leaves,
            },
            ConsumerQuery::MemberBinding {
                alias: "vars",
                leaf_paths: &valid_leaves,
            },
        ];
        let scanned = css_in_js_consumer_scan(source, path, &queries);
        assert_eq!(scanned.len(), 1);
        assert_eq!(scanned[0].0, 1);
        assert_eq!(scanned[0].1.token_path, "color.primary");
    }

    #[test]
    fn consumer_scan_two_member_queries_same_source() {
        // Two definers imported under different aliases with an overlapping leaf
        // path; each read attributes to the alias (query index) it used.
        let source = "const a = brand.color.primary;\nconst b = accent.color.primary;";
        let path = Path::new("card.ts");
        let brand_leaves = leaves(&["color.primary"]);
        let accent_leaves = leaves(&["color.primary"]);
        let queries = [
            ConsumerQuery::MemberBinding {
                alias: "brand",
                leaf_paths: &brand_leaves,
            },
            ConsumerQuery::MemberBinding {
                alias: "accent",
                leaf_paths: &accent_leaves,
            },
        ];
        let scanned = css_in_js_consumer_scan(source, path, &queries);
        assert_eq!(scanned.len(), 2);
        assert!(scanned.contains(&(
            0,
            TokenConsumerHit {
                token_path: "color.primary".to_string(),
                line: 1,
            }
        )));
        assert!(scanned.contains(&(
            1,
            TokenConsumerHit {
                token_path: "color.primary".to_string(),
                line: 2,
            }
        )));
    }
}
