//! CSS-in-JS design-token DEFINITION walker for the design-token blast-radius
//! (CSS program Phase 3d).
//!
//! The zero-runtime CSS-in-JS libraries declare design tokens as a JS OBJECT
//! passed to a library call, binding the token surface to an exported identifier
//! that consumers read via member access (`import { vars } from './tokens';
//! vars.color.primary`). This module is the DEFINITION half of the token
//! blast-radius: it parses JS/TS with oxc, gates recognition on import-binding
//! provenance (reusing `css_in_js_object::module_library`), and for each
//! recognized token-definition call emits the access BINDING plus the flattened
//! dotted LEAF token paths (with each leaf's source line). The CONSUMER half (who
//! reads `vars.color.primary` across modules) is resolved in the analyze layer
//! against the module graph; this walker only produces the defined-token side.
//!
//! Health-time-only, like the 3b/3c CSS-in-JS lifters: it runs over file SOURCE
//! and persists nothing to the extraction cache (no `CACHE_VERSION` bump).
//!
//! # Recognized definition shapes (cut 1: StyleX + vanilla-extract)
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
//!
//! The two CONTRACT-IMPLEMENTATION forms are deliberately NOT definition sites
//! here, because the contract they fill was already declared by
//! `createThemeContract` (captured above) and that is the binding consumers read:
//! - `createTheme(contract, {...})` (2-arg) returns a class string; tokens fill
//!   the existing `contract`.
//! - `createGlobalTheme(selector, contract, {...})` (3-arg) returns void.
//!
//! Panda (`defineTokens` / `token('...')`) is deferred to a 3e follow-on (its
//! dominant consumption is bare-string-in-style-value, a different consumer scan).

use std::path::Path;

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, BindingPattern, ComputedMemberExpression, Expression, ImportDeclarationSpecifier,
    ObjectExpression, ObjectPropertyKind, Program, Statement, StaticMemberExpression,
    VariableDeclarator,
};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::css_in_js_object::{Lib, module_library};

/// A single defined design token: its dotted LEAF path relative to the access
/// binding (`color.primary`, or flat `primaryColor` for StyleX), and the 1-based
/// source line of its key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssInJsToken {
    /// Dotted leaf path relative to the binding (e.g. `color.primary`).
    pub path: String,
    /// 1-based line of the token's key in the defining source.
    pub def_line: u32,
}

/// A CSS-in-JS token-definition site: the exported access binding consumers read
/// through (e.g. `vars`) and the flattened leaf tokens it defines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssInJsTokenDef {
    /// The identifier the token surface is bound to (`vars`), the receiver of
    /// cross-module member access (`vars.color.primary`).
    pub binding: String,
    /// The flattened leaf tokens defined on `binding`.
    pub tokens: Vec<CssInJsToken>,
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
    if alias.is_empty() || leaf_paths.is_empty() {
        return Vec::new();
    }
    let source_type = SourceType::from_path(path).unwrap_or_default();
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, source_type).parse();
    let mut collector = ConsumerCollector {
        source,
        alias,
        leaf_paths,
        hits: Vec::new(),
    };
    collector.visit_program(&ret.program);
    collector.hits
}

/// Walks a consuming program for member accesses on a token binding alias.
struct ConsumerCollector<'a, 'b> {
    source: &'a str,
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
                self.hits.push(TokenConsumerHit {
                    token_path,
                    line: line_at(self.source, span_start),
                });
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
}

/// Collects token-definition sites, gated on import provenance.
struct TokenDefCollector<'a> {
    source: &'a str,
    /// local-binding name -> (library, canonical role). Mirrors the
    /// `css_in_js_object` provenance map but for token-definition roles.
    imports: FxHashMap<&'a str, (Lib, &'a str)>,
    defs: Vec<CssInJsTokenDef>,
}

impl<'a> TokenDefCollector<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
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
        let single = |tokens_arg| {
            Some(Recognized {
                binding_source: BindingSource::LhsIdent,
                tokens_arg,
            })
        };
        match (lib, role) {
            // `defineVars(obj)` / `createThemeContract(obj)`: binding = the assigned
            // identifier, token object = arg 0.
            (Lib::StyleX, "defineVars") | (Lib::VanillaExtract, "createThemeContract")
                if arg_count >= 1 =>
            {
                single(0)
            }
            // 1-arg createTheme returns [themeClass, vars]; tokens on the second
            // destructure element. The 2-arg (contract, tokens) form fills an
            // existing contract and is skipped (createThemeContract is canonical).
            (Lib::VanillaExtract, "createTheme") if arg_count == 1 => Some(Recognized {
                binding_source: BindingSource::TupleElement(1),
                tokens_arg: 0,
            }),
            // 2-arg createGlobalTheme(selector, tokens) returns the vars object;
            // the 3-arg (selector, contract, tokens) form returns void (contract
            // canonical), so only the 2-arg form is a definition site here.
            (Lib::VanillaExtract, "createGlobalTheme") if arg_count == 2 => single(1),
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
        self.collect_leaves(obj, "", &mut tokens);
        if tokens.is_empty() {
            return;
        }
        self.defs.push(CssInJsTokenDef {
            binding: binding.to_owned(),
            tokens,
        });
    }

    /// Flatten an object literal into dotted LEAF paths. An inline-object value
    /// recurses (an intermediate token GROUP, not a token); a value-producing
    /// expression (string / number / `null` contract leaf / call like
    /// `px(2 * grid)` / template / member access like `colors.red['500']`) is a
    /// LEAF token. A BARE IDENTIFIER value (`palette: tailwindPalette`) is SKIPPED:
    /// it references something whose structure is invisible here, most often an
    /// imported token GROUP (recording it as a leaf would invent a phantom token
    /// and wrongly credit every `vars.palette.<x>` access to it); the rarer
    /// identifier-scalar leaf is a lower-bound miss. Spreads and computed keys are
    /// skipped (cannot resolve statically).
    fn collect_leaves(
        &self,
        obj: &ObjectExpression<'a>,
        prefix: &str,
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
                Expression::ObjectExpression(nested) => self.collect_leaves(nested, &path, out),
                // A bare identifier is an unresolvable reference, usually an imported
                // token group; do not record it as a leaf (avoids a phantom token).
                Expression::Identifier(_) => {}
                _ => out.push(CssInJsToken {
                    path,
                    def_line: line_at(self.source, prop.key.span().start),
                }),
            }
        }
    }
}

impl<'a> Visit<'a> for TokenDefCollector<'a> {
    fn visit_variable_declarator(&mut self, decl: &VariableDeclarator<'a>) {
        self.process_declarator(decl);
        walk::walk_variable_declarator(self, decl);
    }
}

/// 1-based line number of a byte offset in `source`.
fn line_at(source: &str, offset: u32) -> u32 {
    let end = (offset as usize).min(source.len());
    1 + u32::try_from(source[..end].bytes().filter(|&b| b == b'\n').count()).unwrap_or(u32::MAX - 1)
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

    #[test]
    fn stylex_define_vars_flat_namespace_call() {
        let d = defs(
            r"
import * as stylex from '@stylexjs/stylex';
export const vars = stylex.defineVars({ primaryColor: '#3b82f6', spacingSm: '4px' });
",
        );
        assert_eq!(paths(&d, "vars"), vec!["primaryColor", "spacingSm"]);
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
}
