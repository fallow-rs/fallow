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
//!   `defineVars({...})` (named import). Binding = the assigned identifier; StyleX
//!   token objects are typically FLAT (depth-1 paths like `primaryColor`).
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
use oxc_ast::ast::{
    Argument, BindingPattern, ComputedMemberExpression, Expression, ImportDeclarationSpecifier,
    NumericLiteral, ObjectExpression, ObjectPropertyKind, Program, Statement,
    StaticMemberExpression, UnaryOperator, VariableDeclarator,
};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};
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

/// Walk a consuming JS/TS source for cross-module reads of a token binding,
/// returning the located reads that resolve to a DEFINED leaf token path. The
/// caller supplies the local `alias` the consuming file imported the token binding
/// under (so aliased imports work) and the set of defined leaf paths. A member
/// access `<alias>.a.b` is a hit when `a.b` is exactly a defined leaf path;
/// intermediate groups (`<alias>.a` where only `a.b` is defined) and accesses on
/// other bindings are not hits, so there is no double-count and no false match.
#[must_use]
#[expect(
    clippy::implicit_hasher,
    reason = "callers build an FxHashSet; std HashSet is a disallowed type here"
)]
pub fn css_in_js_token_consumers(
    source: &str,
    path: &Path,
    alias: &str,
    leaf_paths: &FxHashSet<String>,
) -> Vec<TokenConsumerHit> {
    css_in_js_consumer_scan(
        source,
        path,
        &[ConsumerQuery::MemberBinding { alias, leaf_paths }],
    )
    .into_iter()
    .map(|(_, hit)| hit)
    .collect()
}

/// Walk a consuming JS/TS source for PandaCSS `token('path.to.token')` calls.
/// The caller supplies the local alias imported from Panda's generated
/// `styled-system` token module and the set of defined leaf paths.
#[must_use]
#[expect(
    clippy::implicit_hasher,
    reason = "callers build an FxHashSet; std HashSet is a disallowed type here"
)]
pub fn panda_token_call_consumers(
    source: &str,
    path: &Path,
    alias: &str,
    leaf_paths: &FxHashSet<String>,
) -> Vec<TokenConsumerHit> {
    css_in_js_consumer_scan(
        source,
        path,
        &[ConsumerQuery::PandaTokenCall { alias, leaf_paths }],
    )
    .into_iter()
    .map(|(_, hit)| hit)
    .collect()
}

/// Walk a consuming JS/TS source for common PandaCSS style calls whose object
/// literal values statically name token paths.
#[must_use]
#[expect(
    clippy::implicit_hasher,
    reason = "callers build FxHashSet values; std HashSet is a disallowed type here"
)]
pub fn panda_style_value_consumers(
    source: &str,
    path: &Path,
    aliases: &FxHashSet<String>,
    leaf_paths: &FxHashSet<String>,
) -> Vec<TokenConsumerHit> {
    css_in_js_consumer_scan(
        source,
        path,
        &[ConsumerQuery::PandaStyleValues {
            aliases,
            leaf_paths,
        }],
    )
    .into_iter()
    .map(|(_, hit)| hit)
    .collect()
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

/// Walk a consuming JS/TS source for styled-components / Emotion theme reads such
/// as `theme.colors.brand` and `props.theme.colors.brand`.
#[must_use]
#[expect(
    clippy::implicit_hasher,
    reason = "callers build an FxHashSet; std HashSet is a disallowed type here"
)]
pub fn css_in_js_theme_consumers(
    source: &str,
    path: &Path,
    leaf_paths: &FxHashSet<String>,
) -> Vec<TokenConsumerHit> {
    css_in_js_consumer_scan(source, path, &[ConsumerQuery::ThemeReads { leaf_paths }])
        .into_iter()
        .map(|(_, hit)| hit)
        .collect()
}

/// One attribution query to run against a single parsed consumer source. Each
/// variant mirrors one of the single-query consumer functions above; a scan runs
/// any mix of them against ONE parse of the source.
pub enum ConsumerQuery<'a> {
    /// Member-access reads `<alias>.a.b` of an imported token binding. Mirrors
    /// [`css_in_js_token_consumers`].
    MemberBinding {
        /// The local identifier the token binding was imported under.
        alias: &'a str,
        /// The defined leaf token paths (`color.primary`).
        leaf_paths: &'a FxHashSet<String>,
    },
    /// PandaCSS `token('a.b')` calls through the given alias. Mirrors
    /// [`panda_token_call_consumers`].
    PandaTokenCall {
        /// The local alias imported from Panda's generated token module.
        alias: &'a str,
        /// The defined leaf token paths (`colors.brand`).
        leaf_paths: &'a FxHashSet<String>,
    },
    /// PandaCSS style-call object values naming token paths. Mirrors
    /// [`panda_style_value_consumers`].
    PandaStyleValues {
        /// The local aliases for Panda style calls (`css`, `cva`).
        aliases: &'a FxHashSet<String>,
        /// The defined leaf token paths (`colors.brand`).
        leaf_paths: &'a FxHashSet<String>,
    },
    /// styled-components / Emotion theme reads (`theme.colors.x`). Mirrors
    /// [`css_in_js_theme_consumers`].
    ThemeReads {
        /// The defined leaf token paths (`colors.brand`).
        leaf_paths: &'a FxHashSet<String>,
    },
}

/// Parse `source` once and run every query against the same AST, returning
/// `(query_index, hit)` pairs so the caller can attribute each hit back to the
/// definer that produced its query. Behavior per query is identical to the
/// corresponding single-query function, including the empty-alias / empty-leaf
/// short-circuits (a query that would have early-returned simply contributes no
/// hits, without suppressing the other queries).
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
    for (idx, query) in queries.iter().enumerate() {
        run_consumer_query(query, source, &ret.program, idx, &mut out);
    }
    out
}

/// Run one [`ConsumerQuery`] against an already-parsed `program`, tagging each
/// resulting hit with `idx`. The per-variant guards mirror each single-query
/// function's empty-input short-circuit exactly.
fn run_consumer_query<'a>(
    query: &ConsumerQuery<'_>,
    source: &'a str,
    program: &Program<'a>,
    idx: usize,
    out: &mut Vec<(usize, TokenConsumerHit)>,
) {
    match query {
        ConsumerQuery::MemberBinding { alias, leaf_paths } => {
            if alias.is_empty() || leaf_paths.is_empty() {
                return;
            }
            let mut collector = ConsumerCollector {
                lines: LineCounter::new(source),
                alias,
                leaf_paths,
                hits: Vec::new(),
            };
            collector.visit_program(program);
            out.extend(collector.hits.into_iter().map(|hit| (idx, hit)));
        }
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

/// Walks a consuming program for member accesses on a token binding alias.
struct ConsumerCollector<'a, 'b> {
    lines: LineCounter<'a>,
    alias: &'b str,
    leaf_paths: &'b FxHashSet<String>,
    hits: Vec<TokenConsumerHit>,
}

impl<'a> ConsumerCollector<'a, '_> {
    /// Record a hit if `(base, segments)` is exactly `<alias>.<leaf>` for a defined
    /// leaf path. A node whose chain is `<alias>.<group>` (an intermediate group)
    /// reconstructs a non-leaf path and is skipped, so each access site yields at
    /// most one hit (no double count from the nested member expressions).
    fn record(&mut self, chain: Option<(&'a str, Vec<&'a str>)>, span_start: u32) {
        if let Some((base, segments)) = chain
            && base == self.alias
            && !segments.is_empty()
        {
            let token_path = segments.join(".");
            if self.leaf_paths.contains(&token_path) {
                let line = self.lines.line_at(span_start);
                self.hits.push(TokenConsumerHit { token_path, line });
            }
        }
    }
}

impl<'a> Visit<'a> for ConsumerCollector<'a, '_> {
    fn visit_static_member_expression(&mut self, member: &StaticMemberExpression<'a>) {
        let mut chain = access_object_chain(&member.object);
        if let Some((_, segments)) = chain.as_mut() {
            segments.push(member.property.name.as_str());
        }
        self.record(chain, member.span().start);
        walk::walk_static_member_expression(self, member);
    }

    fn visit_computed_member_expression(&mut self, member: &ComputedMemberExpression<'a>) {
        // Bracket access with a STATIC string-literal key (`vars.color['gray-100']`):
        // the only way to consume a token whose key is not a valid JS identifier
        // (hyphenated `gray-100`, digit-leading `0x`), which design-token systems use
        // heavily. Non-literal computed keys (`vars.color[k]`) cannot be resolved
        // statically and are skipped (a documented lower-bound miss).
        let mut chain = access_object_chain(&member.object);
        if let (Some((_, segments)), Some(key)) =
            (chain.as_mut(), string_literal_key(&member.expression))
        {
            segments.push(key);
        } else {
            chain = None;
        }
        self.record(chain, member.span().start);
        walk::walk_computed_member_expression(self, member);
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
    fn record(&mut self, chain: Option<(&'a str, Vec<&'a str>)>, span_start: u32) {
        let Some((base, segments)) = chain else {
            return;
        };
        let token_segments: &[&str] = match base {
            "theme" => &segments,
            "props" if segments.first().copied() == Some("theme") => &segments[1..],
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
            segments.push(member.property.name.as_str());
        }
        self.record(chain, member.span().start);
        walk::walk_static_member_expression(self, member);
    }

    fn visit_computed_member_expression(&mut self, member: &ComputedMemberExpression<'a>) {
        let mut chain = access_object_chain(&member.object);
        if let (Some((_, segments)), Some(key)) =
            (chain.as_mut(), string_literal_key(&member.expression))
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
fn access_object_chain<'a>(expr: &Expression<'a>) -> Option<(&'a str, Vec<&'a str>)> {
    match expr {
        Expression::Identifier(id) => Some((id.name.as_str(), Vec::new())),
        Expression::StaticMemberExpression(inner) => {
            let (base, mut segments) = access_object_chain(&inner.object)?;
            segments.push(inner.property.name.as_str());
            Some((base, segments))
        }
        Expression::ComputedMemberExpression(inner) => {
            let (base, mut segments) = access_object_chain(&inner.object)?;
            segments.push(string_literal_key(&inner.expression)?);
            Some((base, segments))
        }
        _ => None,
    }
}

/// The value of a string-literal computed-member key (`['gray-100']`), or `None`
/// for any non-string-literal key (which cannot be resolved statically).
fn string_literal_key<'a>(expr: &Expression<'a>) -> Option<&'a str> {
    match expr {
        Expression::StringLiteral(lit) => Some(lit.value.as_str()),
        _ => None,
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
}

/// Collects token-definition sites, gated on import provenance.
struct TokenDefCollector<'a> {
    lines: LineCounter<'a>,
    /// local-binding name -> (library, canonical role). Mirrors the
    /// `css_in_js_object` provenance map but for token-definition roles.
    imports: FxHashMap<&'a str, (Lib, &'a str)>,
    defs: Vec<CssInJsTokenDef>,
}

impl<'a> TokenDefCollector<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            lines: LineCounter::new(source),
            imports: FxHashMap::default(),
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
                    ImportDeclarationSpecifier::ImportSpecifier(s) => {
                        (s.local.name.as_str(), s.imported.name().as_str())
                    }
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
        let single = |tokens_arg, origin| {
            Some(Recognized {
                binding_source: BindingSource::LhsIdent,
                tokens_arg,
                origin,
            })
        };
        match (lib, role) {
            // `defineVars(obj)` / `createThemeContract(obj)`: binding = the assigned
            // identifier, token object = arg 0.
            (Lib::StyleX, "defineVars") if arg_count >= 1 => single(0, CssInJsTokenOrigin::StyleX),
            (Lib::VanillaExtract, "createThemeContract") if arg_count >= 1 => {
                single(0, CssInJsTokenOrigin::VanillaExtract)
            }
            // 1-arg createTheme returns [themeClass, vars]; tokens on the second
            // destructure element. The 2-arg (contract, tokens) form fills an
            // existing contract and is skipped (createThemeContract is canonical).
            (Lib::VanillaExtract, "createTheme") if arg_count == 1 => Some(Recognized {
                binding_source: BindingSource::TupleElement(1),
                tokens_arg: 0,
                origin: CssInJsTokenOrigin::VanillaExtract,
            }),
            // 2-arg createGlobalTheme(selector, tokens) returns the vars object;
            // the 3-arg (selector, contract, tokens) form returns void (contract
            // canonical), so only the 2-arg form is a definition site here.
            (Lib::VanillaExtract, "createGlobalTheme") if arg_count == 2 => {
                single(1, CssInJsTokenOrigin::VanillaExtract)
            }
            (Lib::Panda, "defineTokens") if arg_count >= 1 => single(0, CssInJsTokenOrigin::Panda),
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
        let Some(Argument::ObjectExpression(obj)) = call.arguments.get(recognized.tokens_arg)
        else {
            return;
        };
        let mut tokens = Vec::new();
        collect_token_leaves(&mut self.lines, obj, "", recognized.origin, &mut tokens);
        if tokens.is_empty() {
            return;
        }
        self.defs.push(CssInJsTokenDef {
            binding: binding.to_owned(),
            origin: recognized.origin,
            tokens,
        });
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
        self.process_declarator(decl);
        walk::walk_variable_declarator(self, decl);
    }

    fn visit_export_default_declaration(
        &mut self,
        decl: &oxc_ast::ast::ExportDefaultDeclaration<'a>,
    ) {
        if let Some(Expression::CallExpression(call)) = decl.declaration.as_expression() {
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
    fn stylex_define_vars_named_import_nested() {
        let d = defs(
            r"
import { defineVars } from '@stylexjs/stylex';
export const vars = defineVars({ color: { primary: '#000', secondary: '#fff' } });
",
        );
        assert_eq!(paths(&d, "vars"), vec!["color.primary", "color.secondary"]);
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
        let hits = css_in_js_theme_consumers(
            r"
import styled from 'styled-components';
export const Card = styled.div`
  color: ${({ theme }) => theme.colors.brand};
  margin: ${props => props.theme.space.card};
`;
",
            Path::new("card.tsx"),
            &leaves,
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
        let src = "import { defineVars } from '@stylexjs/stylex';\nexport const vars = defineVars({\n  color: {\n    primary: '#000',\n    secondary: '#fff',\n  },\n});\n";
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
    fn spread_and_computed_keys_are_skipped() {
        let d = defs(
            r"
import { defineVars } from '@stylexjs/stylex';
const base = { a: '1' };
export const vars = defineVars({ ...base, ['x' + 'y']: '2', real: '#000' });
",
        );
        // Only the statically-resolvable `real` leaf survives.
        assert_eq!(paths(&d, "vars"), vec!["real"]);
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

    fn consumers(source: &str, alias: &str, paths: &[&str]) -> Vec<TokenConsumerHit> {
        css_in_js_token_consumers(source, Path::new("card.ts"), alias, &leaves(paths))
    }

    fn panda_consumers(source: &str, alias: &str, paths: &[&str]) -> Vec<TokenConsumerHit> {
        panda_token_call_consumers(source, Path::new("card.ts"), alias, &leaves(paths))
    }

    fn panda_style_consumers(
        source: &str,
        aliases: &[&str],
        paths: &[&str],
    ) -> Vec<TokenConsumerHit> {
        let aliases = aliases.iter().map(|s| (*s).to_string()).collect();
        panda_style_value_consumers(source, Path::new("card.ts"), &aliases, &leaves(paths))
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
    fn consumer_other_binding_not_matched() {
        // A same-named member access on a DIFFERENT binding must not be a hit.
        let hits = consumers("const a = other.color.primary;", "vars", &["color.primary"]);
        assert!(hits.is_empty());
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

        let individual: Vec<(usize, TokenConsumerHit)> =
            css_in_js_token_consumers(source, path, "vars", &member_leaves)
                .into_iter()
                .map(|hit| (0, hit))
                .chain(
                    panda_token_call_consumers(source, path, "token", &panda_call_leaves)
                        .into_iter()
                        .map(|hit| (1, hit)),
                )
                .chain(
                    panda_style_value_consumers(
                        source,
                        path,
                        &panda_style_aliases,
                        &panda_style_leaves,
                    )
                    .into_iter()
                    .map(|hit| (2, hit)),
                )
                .chain(
                    css_in_js_theme_consumers(source, path, &theme_leaves)
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
