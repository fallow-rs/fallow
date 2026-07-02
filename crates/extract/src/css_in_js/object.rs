//! CSS-in-JS OBJECT-notation lifter for the styling-health analytics pipeline
//! (CSS program Phase 3c).
//!
//! The object-only / zero-runtime camp of CSS-in-JS (vanilla-extract, StyleX,
//! Panda, plus emotion's object form) writes its CSS as a JS OBJECT LITERAL
//! passed to a library call (`style({ color: 'red' })`,
//! `stylex.create({ root: {...} })`, `css({...})`, `styled.div({...})`) rather
//! than a tagged template. The Phase 3b lexical lifter
//! ([`crate::css_in_js::css_in_js_virtual_stylesheet`]) only handles the template
//! form, so an object-notation app (the libraries every new RSC / compile-time
//! project picks) got `null` styling analytics. This module is the object-form
//! analogue: it parses the JS/TS with oxc, walks the AST for import-gated
//! object-literal style calls, and SERIALIZES each style bucket into the SAME
//! blank-line-padded virtual stylesheet 3b emits, so both forms converge on one
//! [`crate::compute_css_analytics`] + styling-health pipeline (no forked metric
//! logic). The object -> CSS transform is unavoidable (it happens in the bundler
//! fallow does not run); the AST just removes the lexing pain and hands us a
//! structured object.
//!
//! It is health-time-only, like 3b: it runs over file SOURCE in the engine's CSS
//! walk and persists nothing to the extraction cache (no `CACHE_VERSION` bump).
//! The second oxc parse it costs (the extraction pass already parsed the file,
//! but that AST is ephemeral and unreachable in the health walk) is bounded by
//! the same dep gate + `--css` gate 3b uses.
//!
//! # Provenance: import-binding, not name (no false positives)
//!
//! `style` / `css` / `cva` are generic names a project may define locally or
//! import from an UNRELATED library (`cva` from `class-variance-authority` is a
//! class-string helper, not CSS). Recognition is therefore gated on IMPORT
//! BINDING: a call only serializes when its callee name was imported from a
//! recognized CSS-in-JS module in THIS file. A local `const style = ...` or a
//! `css` / `cva` from an unrelated package never fires.
//!
//! # Static-only serialization
//!
//! Only static string / number values are emitted (camelCase -> kebab-case,
//! implicit `px` on numbers outside the unitless set, selector-shaped keys become
//! nested rules). DYNAMIC values (identifier / member / call), SPREAD, COMPUTED keys,
//! and objects under a NON-selector key (a `cva` `variants` map, not a style
//! block) are DROPPED, never guessed: there is no JS interpreter and no value
//! evaluation, so a `color: theme.primary` contributes nothing rather than a
//! fabricated token. A bucket that drops to zero static declarations is omitted
//! entirely (no empty synthetic rule).
//!
//! # Three sheets: atomic / structural-partial / structural
//!
//! StyleX and Panda compile to ATOMIC CSS (one declaration per class, flat by
//! construction), so the structure of their lifted source rules is not
//! representative: a flat synthetic rule would trivially score a structural A and
//! dilute a mixed project's `!important` / nesting density. Separately, a bucket
//! that DROPPED a dynamic declaration could collapse onto another bucket's
//! fingerprint (the dropped declaration is exactly what distinguished them), a
//! false duplicate. The serializer therefore returns THREE virtual stylesheets so
//! the engine can apply the right policy to each:
//!
//! - [`CssInJsObjectSheets::structural`]: vanilla-extract + emotion buckets with
//!   NO dropped declarations. Full analytics, including duplicate-block
//!   fingerprints and the styling-health structural grade inputs.
//! - [`CssInJsObjectSheets::structural_partial`]: vanilla-extract + emotion
//!   buckets that dropped a dynamic / spread / computed declaration. Their tokens
//!   and metrics still count, but the engine suppresses their duplicate-block
//!   fingerprints (a dropped declaration could have distinguished two otherwise
//!   identical blocks).
//! - [`CssInJsObjectSheets::atomic`]: StyleX + Panda buckets. Token-sprawl only;
//!   the engine excludes them from the structural grade inputs and from
//!   duplicate-block fingerprints (flat by construction; their structure is a
//!   build-output property, not authored).
//!
//! Note: numeric values outside the unitless set gain a synthetic `px` the author
//! did not literally type (`fontSize: 14` -> `font-size: 14px`); this is correct
//! for the font-size-unit-MIX smell (a unit IS implied) but the synthesized unit
//! is an analytic convenience, not authored text.

use std::path::Path;

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, Expression, ImportDeclarationSpecifier, NumericLiteral, ObjectExpression,
    ObjectPropertyKind, Program, PropertyKey, Statement, UnaryOperator,
};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};
use rustc_hash::FxHashMap;

use super::shared::{WRAPPER, count_newlines};

/// CSS property names (camelCase) whose numeric values are UNITLESS: a bare
/// number is the value, not a `px` length. Mirrors React's well-known unitless
/// set (`CSSProperty.js`), so `lineHeight: 1.5` -> `line-height: 1.5` while
/// `padding: 8` -> `padding: 8px`. Comparison is against the camelCase key as
/// authored (before kebab conversion).
const UNITLESS_PROPERTIES: &[&str] = &[
    "animationIterationCount",
    "aspectRatio",
    "borderImageOutset",
    "borderImageSlice",
    "borderImageWidth",
    "boxFlex",
    "boxFlexGroup",
    "boxOrdinalGroup",
    "columnCount",
    "columns",
    "flex",
    "flexGrow",
    "flexPositive",
    "flexShrink",
    "flexNegative",
    "flexOrder",
    "gridArea",
    "gridRow",
    "gridRowEnd",
    "gridRowSpan",
    "gridRowStart",
    "gridColumn",
    "gridColumnEnd",
    "gridColumnSpan",
    "gridColumnStart",
    "fontWeight",
    "lineClamp",
    "lineHeight",
    "opacity",
    "order",
    "orphans",
    "scale",
    "tabSize",
    "widows",
    "zIndex",
    "zoom",
    "fillOpacity",
    "floodOpacity",
    "stopOpacity",
    "strokeDasharray",
    "strokeDashoffset",
    "strokeMiterlimit",
    "strokeOpacity",
    "strokeWidth",
];

/// The recognized object-notation CSS-in-JS libraries. The atomic split drives
/// whether a library's synthetic rules count toward the styling-health structural
/// grade and duplicate-block fingerprints.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Lib {
    /// vanilla-extract (`@vanilla-extract/css` / `/recipes`): real selectors via
    /// `globalStyle` / `selectors`, structure is meaningful.
    VanillaExtract,
    /// emotion `css(...)` object form (`@emotion/react` / `@emotion/css`).
    Emotion,
    /// emotion `styled.div(...)` object form (`@emotion/styled`); member calls.
    EmotionStyled,
    /// StyleX (`@stylexjs/stylex`): compile-time atomic CSS, flat by construction.
    StyleX,
    /// Panda (`styled-system` codegen, gated on `@pandacss/dev`): atomic CSS.
    Panda,
}

impl Lib {
    /// Whether the library compiles to flat atomic CSS whose source-rule
    /// structure is not representative (excluded from the styling-health
    /// structural grade and duplicate fingerprints).
    const fn is_atomic(self) -> bool {
        matches!(self, Self::StyleX | Self::Panda)
    }
}

/// The three virtual stylesheets lifted from a source's object-notation
/// CSS-in-JS, each blank-line-padded so CSS metric line numbers map back onto the
/// real source. Each is `None` when the source has no object CSS-in-JS of that
/// class (so callers skip it; no `files_analyzed` inflation). See the module docs
/// for the per-sheet engine policy.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CssInJsObjectSheets {
    /// vanilla-extract + emotion buckets with no dropped declarations: full
    /// analytics incl. duplicate fingerprints + structural grade inputs.
    pub structural: Option<String>,
    /// vanilla-extract + emotion buckets that dropped a dynamic declaration:
    /// tokens + metrics count, duplicate fingerprints suppressed by the engine.
    pub structural_partial: Option<String>,
    /// StyleX + Panda atomic buckets: token-sprawl only; excluded from the
    /// structural grade inputs and duplicate fingerprints.
    pub atomic: Option<String>,
}

impl CssInJsObjectSheets {
    /// Whether all three sheets are empty (no recognized object CSS-in-JS).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.structural.is_none() && self.structural_partial.is_none() && self.atomic.is_none()
    }
}

/// Which sheet a lifted bucket belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stream {
    Structural,
    StructuralPartial,
    Atomic,
}

/// A single lifted style bucket awaiting emission: the byte offset to pad to (the
/// property key for a multi-bucket call, so duplicate / notable findings land on
/// the right line), the serialized rule (`<selector>{<decls>}`), and its sheet.
struct Bucket {
    offset: u32,
    rule: String,
    stream: Stream,
}

/// Lift the object-notation CSS-in-JS in a JS/TS source into the structural /
/// structural-partial / atomic virtual stylesheets. Parses with oxc (source type
/// inferred from `path`), maps import bindings to recognized libraries, walks for
/// style calls, serializes each bucket, and pads each to its source line. All
/// sheets are `None` when the source has no recognized object CSS-in-JS import.
#[must_use]
pub fn css_in_js_object_sheets(source: &str, path: &Path) -> CssInJsObjectSheets {
    let source_type = SourceType::from_path(path).unwrap_or_default();
    let allocator = Allocator::default();
    // A best-effort parse: even with recoverable syntax errors oxc returns a
    // partial program, and the walk lifts whatever object styles it can reach
    // (matching `compute_css_analytics`'s error-recovery philosophy).
    let ret = Parser::new(&allocator, source, source_type).parse();

    let mut collector = ObjectStyleCollector::new(source);
    collector.build_import_map(&ret.program);
    if collector.imports.is_empty() {
        // No recognized CSS-in-JS import binding: provenance gate is closed, so
        // nothing can fire. Cheap exit before the call walk.
        return CssInJsObjectSheets::default();
    }
    collector.visit_program(&ret.program);
    collector.finish()
}

/// Walks a parsed program collecting object-notation style buckets, gated on
/// import provenance.
struct ObjectStyleCollector<'a> {
    source: &'a str,
    /// local-binding name -> (library, canonical function role). The role is the
    /// IMPORTED (canonical) name for a named import, so an alias
    /// (`import { style as s }`) still dispatches on `style`; the local name for a
    /// default / namespace binding (those route through the member-call arms,
    /// where only the library matters).
    imports: FxHashMap<&'a str, (Lib, &'a str)>,
    buckets: Vec<Bucket>,
}

impl<'a> ObjectStyleCollector<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            imports: FxHashMap::default(),
            buckets: Vec::new(),
        }
    }

    /// Map each import binding from a recognized CSS-in-JS module to its library.
    /// Named bindings (`import { style } from '@vanilla-extract/css'`) map the
    /// local alias; default / namespace bindings (`import stylex from
    /// '@stylexjs/stylex'`, `import styled from '@emotion/styled'`) map the
    /// binding for later member-call recognition (`stylex.create`, `styled.div`).
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
                    // A named import dispatches on its CANONICAL imported name, so
                    // `import { style as s }` still matches the `style` arm.
                    ImportDeclarationSpecifier::ImportSpecifier(s) => {
                        (s.local.name.as_str(), s.imported.name().as_str())
                    }
                    // A default import routes through the member-call / call arms.
                    // For emotion the default export IS the `css` function, so
                    // canonicalize its role to `css` and let any local alias fire
                    // (mirrors how EmotionStyled member calls ignore the binding
                    // name). Other libs keep the local name for member-call
                    // recognition (`import stylex from ...` -> `stylex.create`).
                    ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                        let role = if lib == Lib::Emotion {
                            "css"
                        } else {
                            s.local.name.as_str()
                        };
                        (s.local.name.as_str(), role)
                    }
                    ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                        (s.local.name.as_str(), s.local.name.as_str())
                    }
                };
                self.imports.insert(local, (lib, role));
            }
        }
    }

    fn finish(self) -> CssInJsObjectSheets {
        let source = self.source;
        let mut buckets = self.buckets;
        // Emit in source order so the incremental blank-line padding only ever
        // moves forward (the AST walk can surface a nested call before an earlier
        // sibling depending on tree shape).
        buckets.sort_by_key(|b| b.offset);
        CssInJsObjectSheets {
            structural: render(source, &buckets, Stream::Structural),
            structural_partial: render(source, &buckets, Stream::StructuralPartial),
            atomic: render(source, &buckets, Stream::Atomic),
        }
    }

    /// Resolve a call's callee to `(library, kind)` if it is a recognized
    /// object-notation style call. `kind` selects how the arguments become
    /// buckets.
    fn recognize(&self, callee: &Expression<'a>) -> Option<(Lib, CallKind)> {
        match callee {
            Expression::Identifier(id) => {
                let (lib, role) = *self.imports.get(id.name.as_str())?;
                let kind = match (lib, role) {
                    // `style(obj)` / `css(obj)`: one object -> one bucket.
                    (Lib::VanillaExtract, "style") | (Lib::Emotion | Lib::Panda, "css") => {
                        CallKind::SingleObject
                    }
                    // `styleVariants({ k: obj })`: one bucket per key.
                    (Lib::VanillaExtract, "styleVariants") => CallKind::ObjectOfObjects,
                    // `globalStyle('sel', obj)`: real-selector rule.
                    (Lib::VanillaExtract, "globalStyle") => CallKind::GlobalStyle,
                    // `recipe({ base, variants })` / `cva({...})`: lift `base` only.
                    (Lib::VanillaExtract, "recipe") | (Lib::Panda, "cva") => CallKind::RecipeBase,
                    _ => return None,
                };
                Some((lib, kind))
            }
            // `styled.div({...})` / `stylex.create({...})`: member call on a bound
            // namespace / default import.
            Expression::StaticMemberExpression(member) => {
                let Expression::Identifier(obj) = &member.object else {
                    return None;
                };
                let (lib, _) = *self.imports.get(obj.name.as_str())?;
                let kind = match (lib, member.property.name.as_str()) {
                    (Lib::EmotionStyled, _) => CallKind::SingleObject,
                    (Lib::StyleX, "create") => CallKind::ObjectOfObjects,
                    _ => return None,
                };
                Some((lib, kind))
            }
            // `styled(Component)({...})`: callee is itself a `styled(...)` call.
            Expression::CallExpression(inner) => {
                let Expression::Identifier(id) = &inner.callee else {
                    return None;
                };
                matches!(
                    self.imports.get(id.name.as_str()),
                    Some((Lib::EmotionStyled, _))
                )
                .then_some((Lib::EmotionStyled, CallKind::SingleObject))
            }
            _ => None,
        }
    }

    /// Turn a recognized call's arguments into buckets and record them.
    fn collect_call(&mut self, callee: &Expression<'a>, args: &[Argument<'a>]) {
        let Some((lib, kind)) = self.recognize(callee) else {
            return;
        };
        let atomic = lib.is_atomic();
        match kind {
            CallKind::SingleObject => {
                if let Some(obj) = object_arg(args, 0) {
                    self.push_bucket(obj, WRAPPER, atomic, obj.span().start);
                }
            }
            CallKind::ObjectOfObjects => {
                // `stylex.create({ root: {...} })` / `styleVariants({ a: {...} })`:
                // one bucket per key (padded to the key line). Only the
                // single-object form; the functional `styleVariants(data, fn)`
                // overload returns styles dynamically and is skipped.
                if args.len() != 1 {
                    return;
                }
                let Some(obj) = object_arg(args, 0) else {
                    return;
                };
                for prop in &obj.properties {
                    if let ObjectPropertyKind::ObjectProperty(p) = prop
                        && let Expression::ObjectExpression(inner) = &p.value
                    {
                        self.push_bucket(inner, WRAPPER, atomic, p.key.span().start);
                    }
                }
            }
            CallKind::RecipeBase => {
                // `recipe({ base: {...}, variants: {...} })` / `cva({...})`: only
                // the `base` style object is plain declarations; `variants` /
                // `compoundVariants` / `defaultVariants` are config maps, not style
                // blocks, and are skipped (deferred).
                let Some(obj) = object_arg(args, 0) else {
                    return;
                };
                for prop in &obj.properties {
                    if let ObjectPropertyKind::ObjectProperty(p) = prop
                        && static_key(&p.key).as_deref() == Some("base")
                        && let Expression::ObjectExpression(inner) = &p.value
                    {
                        self.push_bucket(inner, WRAPPER, atomic, p.key.span().start);
                    }
                }
            }
            CallKind::GlobalStyle => {
                // `globalStyle('selector', { ... })`: real selector, structural.
                let (Some(selector), Some(obj)) = (string_arg(args, 0), object_arg(args, 1)) else {
                    return;
                };
                let selector = sanitize_selector(&selector);
                if !selector.is_empty() {
                    self.push_bucket(obj, &selector, atomic, obj.span().start);
                }
            }
        }
    }

    /// Serialize one object literal into a `<selector>{<decls>}` rule and record
    /// it, dropping the bucket when no static declaration survives and routing it
    /// to the right sheet (atomic, or structural / structural-partial by whether
    /// any declaration was dropped).
    fn push_bucket(
        &mut self,
        obj: &ObjectExpression<'a>,
        selector: &str,
        atomic: bool,
        offset: u32,
    ) {
        let mut body = String::new();
        let mut dropped = false;
        serialize_object_body(obj, &mut body, &mut dropped);
        if body.is_empty() {
            return;
        }
        let stream = if atomic {
            Stream::Atomic
        } else if dropped {
            Stream::StructuralPartial
        } else {
            Stream::Structural
        };
        self.buckets.push(Bucket {
            offset,
            rule: format!("{selector}{{{body}}}"),
            stream,
        });
    }
}

impl<'a> Visit<'a> for ObjectStyleCollector<'a> {
    fn visit_call_expression(&mut self, call: &oxc_ast::ast::CallExpression<'a>) {
        self.collect_call(&call.callee, &call.arguments);
        walk::walk_call_expression(self, call);
    }
}

/// Render the buckets of one stream into a blank-line-padded sheet, or `None` if
/// there are none. Each bucket is padded to its source line so CSS metric line
/// numbers map back onto the source.
fn render(source: &str, buckets: &[Bucket], stream: Stream) -> Option<String> {
    let mut out = String::new();
    let mut current_line: usize = 1;
    let mut found = false;
    for bucket in buckets.iter().filter(|b| b.stream == stream) {
        let block_line = 1 + count_newlines(&source[..bucket.offset as usize]);
        while current_line < block_line {
            out.push('\n');
            current_line += 1;
        }
        out.push_str(&bucket.rule);
        current_line += count_newlines(&bucket.rule);
        found = true;
    }
    found.then_some(out)
}

/// How a recognized call's arguments map to style buckets.
enum CallKind {
    /// The first object argument is one style bucket (`style(obj)`, `css(obj)`,
    /// `styled.div(obj)`).
    SingleObject,
    /// The first object argument is a map of key -> style object; each value
    /// object is its own bucket (`stylex.create({...})`, `styleVariants({...})`).
    ObjectOfObjects,
    /// The first object argument is a recipe (`{ base, variants, ... }`); only
    /// `base` is a style bucket (`recipe({...})`, `cva({...})`).
    RecipeBase,
    /// `globalStyle('selector', obj)`: the second arg is a style bucket emitted
    /// under the real first-arg selector.
    GlobalStyle,
}

/// The recognized library for an import module specifier, or `None`. Panda's
/// runtime `css` / `cva` is imported from a generated `styled-system` path rather
/// than a package name, so any specifier whose path contains a `styled-system`
/// segment is treated as Panda (still behind the engine's `@pandacss/dev` dep
/// gate, which decides whether the file is scanned at all).
pub(super) fn module_library(specifier: &str) -> Option<Lib> {
    match specifier {
        "@pandacss/dev" => Some(Lib::Panda),
        "@vanilla-extract/css" | "@vanilla-extract/recipes" => Some(Lib::VanillaExtract),
        "@emotion/react" | "@emotion/css" => Some(Lib::Emotion),
        "@emotion/styled" => Some(Lib::EmotionStyled),
        "@stylexjs/stylex" => Some(Lib::StyleX),
        _ if specifier
            .split(['/', '\\'])
            .any(|segment| segment == "styled-system") =>
        {
            Some(Lib::Panda)
        }
        _ => None,
    }
}

/// The object-expression argument at `index`, if present and an object literal.
fn object_arg<'a, 'b>(args: &'b [Argument<'a>], index: usize) -> Option<&'b ObjectExpression<'a>> {
    match args.get(index) {
        Some(Argument::ObjectExpression(obj)) => Some(obj),
        _ => None,
    }
}

/// The string-literal argument at `index`, if present.
fn string_arg(args: &[Argument<'_>], index: usize) -> Option<String> {
    match args.get(index) {
        Some(Argument::StringLiteral(lit)) => Some(lit.value.to_string()),
        _ => None,
    }
}

/// Serialize an object literal's static declarations into a CSS rule body. A
/// selector-shaped key with an object value (`:hover`, `&:hover`, `@media ...`,
/// vanilla-extract `selectors: {...}`) becomes a nested rule and recurses through
/// further selector-shaped keys, so authored selector nesting depth is reflected
/// (a real structural signal); dynamic values, spreads, computed keys, and
/// objects under a NON-selector key (a `cva` `variants` map) are dropped and flip
/// `dropped`.
fn serialize_object_body(obj: &ObjectExpression<'_>, out: &mut String, dropped: &mut bool) {
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            // Spread (`...base`) carries no statically-known declarations.
            *dropped = true;
            continue;
        };
        let Some(key) = static_key(&prop.key) else {
            // Computed key (`[prop]: v`): cannot be resolved statically.
            *dropped = true;
            continue;
        };
        match &prop.value {
            Expression::ObjectExpression(nested) if is_selector_key(&key) => {
                serialize_nested(&key, nested, out, dropped);
            }
            Expression::ObjectExpression(_) => {
                // An object under a non-selector key (`variants: {...}`, a StyleX
                // conditional value): not a style block, drop it.
                *dropped = true;
            }
            value => {
                if let Some(rendered) = serialize_value(&key, value) {
                    out.push_str(&rendered);
                } else {
                    *dropped = true;
                }
            }
        }
    }
}

/// Serialize a nested object under a selector- or at-rule-shaped key into a
/// nested rule (one level). vanilla-extract's `selectors: { '&:hover': {...} }`
/// wrapper is unwrapped so each inner selector becomes its own nested rule.
fn serialize_nested(
    key: &str,
    nested: &ObjectExpression<'_>,
    out: &mut String,
    dropped: &mut bool,
) {
    // `selectors: { '&:hover': {...}, ... }` is a wrapper, not a selector: emit
    // each inner key as its own nested rule.
    if key == "selectors" {
        for prop in &nested.properties {
            match prop {
                ObjectPropertyKind::ObjectProperty(p) => {
                    if let (Some(inner_key), Expression::ObjectExpression(inner)) =
                        (static_key(&p.key), &p.value)
                    {
                        serialize_nested(&inner_key, inner, out, dropped);
                    } else {
                        *dropped = true;
                    }
                }
                ObjectPropertyKind::SpreadProperty(_) => *dropped = true,
            }
        }
        return;
    }

    let mut body = String::new();
    serialize_object_body(nested, &mut body, dropped);
    if body.is_empty() {
        return;
    }
    out.push_str(&nested_selector(key));
    out.push('{');
    out.push_str(&body);
    out.push('}');
}

/// Whether an object-property key introduces a nested SELECTOR / at-rule (so its
/// object value is a nested rule) rather than a CSS property. Selector-shaped:
/// the vanilla-extract `selectors` wrapper, an at-rule (`@media`), or a key
/// starting with a selector character (`:`, `&`, a combinator, `.`, `#`, `[`, `*`,
/// or a leading space for a descendant). A plain CSS property name (`color`,
/// `backgroundColor`, `--custom`) is NOT a selector.
fn is_selector_key(key: &str) -> bool {
    if key == "selectors" {
        return true;
    }
    matches!(
        key.trim_start().chars().next(),
        Some(':' | '&' | '@' | '>' | '+' | '~' | '.' | '#' | '[' | '*')
    ) || key.starts_with(' ')
}

/// Map a nested object key to a CSS nested-rule prelude. At-rule keys
/// (`@media ...`) and `&`-anchored selectors pass through; a bare pseudo /
/// selector is prefixed with `&` so it parses as relative nesting.
fn nested_selector(key: &str) -> String {
    let trimmed = key.trim();
    if trimmed.starts_with('@') || trimmed.starts_with('&') {
        return trimmed.to_string();
    }
    format!("&{trimmed}")
}

/// Render a single static declaration `key: value` (with trailing `;`), or `None`
/// when the value is not a static string / number (dynamic values are dropped).
fn serialize_value(key: &str, value: &Expression<'_>) -> Option<String> {
    let rendered = static_value(key, value)?;
    Some(format!("{}:{rendered};", kebab_case(key)))
}

/// The CSS text of a static string / number expression for property `key`, or
/// `None` for any dynamic / non-literal value. Numbers outside the unitless set
/// gain an implicit `px`; negative numbers (`-8` as a unary minus) are handled.
fn static_value(key: &str, value: &Expression<'_>) -> Option<String> {
    match value {
        Expression::StringLiteral(lit) => {
            let text = lit.value.as_str().trim();
            (!text.is_empty()).then(|| text.to_string())
        }
        Expression::NumericLiteral(num) => Some(render_number(key, num)),
        Expression::UnaryExpression(unary) if unary.operator == UnaryOperator::UnaryNegation => {
            if let Expression::NumericLiteral(num) = &unary.argument {
                Some(format!("-{}", render_number(key, num)))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Render a numeric literal for property `key`, appending `px` unless the
/// property is unitless, a custom property, or the value is zero. The number is
/// rendered from its PARSED value, not the raw source text, so a hex / octal /
/// binary / scientific literal (`0xFF`, `1e3`) becomes a valid CSS decimal
/// (`255`, `1000`) rather than a non-CSS token; `format_f64` preserves `1.5` /
/// `700` exactly. The custom-property carve-out matches the real compiled output:
/// emotion (`!isCustomProperty(key)` in `@emotion/serialize`) and React both
/// leave a numeric `--x` value unitless, so adding `px` would fabricate a unit
/// the bundler never emits.
fn render_number(key: &str, num: &NumericLiteral<'_>) -> String {
    let value = format_f64(num.value);
    if is_unitless(key) || key.starts_with("--") || num.value == 0.0 {
        value
    } else {
        format!("{value}px")
    }
}

fn format_f64(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

/// Whether `key` (camelCase) is a unitless CSS property.
fn is_unitless(key: &str) -> bool {
    UNITLESS_PROPERTIES.contains(&key)
}

/// The static name of an object-property key (string literal or identifier), or
/// `None` for a computed / dynamic key.
fn static_key(key: &PropertyKey<'_>) -> Option<String> {
    key.static_name().map(|name| name.to_string())
}

/// Convert a camelCase CSS property name to kebab-case. A leading uppercase
/// (vendor prefix `WebkitBoxShadow`) becomes a leading `-` (`-webkit-box-shadow`),
/// and the lowercase `ms` Microsoft prefix (`msFlexAlign`, the one React/emotion
/// write lowercase) becomes `-ms-`. Custom properties (`--x`) and already-kebab
/// names pass through unchanged.
fn kebab_case(name: &str) -> String {
    if name.starts_with("--") || name.contains('-') {
        return name.to_string();
    }
    let mut out = String::with_capacity(name.len() + 2);
    // The `ms` vendor prefix is authored lowercase (unlike `Webkit`/`Moz`/`O`),
    // so prepend the leading `-` an uppercase boundary would otherwise add
    // (`msTransform` -> `-ms-transform`).
    if let Some(rest) = name.strip_prefix("ms")
        && rest.chars().next().is_some_and(|c| c.is_ascii_uppercase())
    {
        out.push('-');
    }
    for ch in name.chars() {
        if ch.is_ascii_uppercase() {
            out.push('-');
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// Drop any character from a `globalStyle` selector that could break out of the
/// synthetic rule context (`{`, `}`) or split a declaration (`;`). The selector
/// is authored CSS, kept as-is otherwise so its specificity / complexity are
/// measured for real.
fn sanitize_selector(selector: &str) -> String {
    selector
        .chars()
        .filter(|&c| c != '{' && c != '}' && c != ';')
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;
    use crate::compute_css_analytics;

    fn sheets(source: &str) -> CssInJsObjectSheets {
        css_in_js_object_sheets(source, Path::new("styles.ts"))
    }

    #[test]
    fn vanilla_extract_style_lifts_to_parseable_css() {
        let src = "import { style } from '@vanilla-extract/css';\n\
                   export const box = style({\n\
                   backgroundColor: 'red',\n\
                   padding: 8,\n\
                   });\n";
        let s = sheets(src);
        let css = s.structural.expect("vanilla-extract style is structural");
        // camelCase -> kebab, implicit px on the numeric value.
        assert!(css.contains("background-color:red;"), "css={css:?}");
        assert!(css.contains("padding:8px;"), "px default: css={css:?}");
        let a = compute_css_analytics(&css).expect("lifted CSS parses");
        assert!(a.total_declarations >= 2, "declarations counted: {a:?}");
        assert!(s.atomic.is_none(), "vanilla-extract is not atomic");
    }

    #[test]
    fn unitless_properties_keep_bare_number() {
        let src = "import { style } from '@vanilla-extract/css';\n\
                   const x = style({ lineHeight: 1.5, zIndex: 10, fontWeight: 700, padding: 4 });\n";
        let css = sheets(src).structural.expect("structural");
        assert!(css.contains("line-height:1.5;"), "css={css:?}");
        assert!(css.contains("z-index:10;"), "css={css:?}");
        assert!(css.contains("font-weight:700;"), "css={css:?}");
        assert!(css.contains("padding:4px;"), "css={css:?}");
    }

    #[test]
    fn one_level_nesting_via_relative_selector() {
        let src = "import { style } from '@vanilla-extract/css';\n\
                   const x = style({ color: 'red', ':hover': { color: 'blue' } });\n";
        let css = sheets(src).structural.expect("structural");
        assert!(
            css.contains("&:hover{color:blue;}"),
            "nested rule: css={css:?}"
        );
        let a = compute_css_analytics(&css).expect("nested parses");
        assert!(a.rule_count >= 2, "nested rule counted: {a:?}");
    }

    #[test]
    fn vanilla_extract_selectors_wrapper_unwrapped() {
        let src = "import { style } from '@vanilla-extract/css';\n\
                   const x = style({ color: 'red', selectors: { '&:hover': { color: 'blue' } } });\n";
        let css = sheets(src).structural.expect("structural");
        assert!(
            css.contains("&:hover{color:blue;}"),
            "selectors wrapper unwrapped: css={css:?}"
        );
        // The `selectors` key itself must NOT become a `&selectors{}` rule.
        assert!(
            !css.contains("selectors{"),
            "no literal selectors rule: css={css:?}"
        );
    }

    #[test]
    fn global_style_keeps_real_selector() {
        let src = "import { globalStyle } from '@vanilla-extract/css';\n\
                   globalStyle('html, body', { margin: 0 });\n";
        let css = sheets(src).structural.expect("structural");
        assert!(
            css.contains("html, body{margin:0;}"),
            "real selector: css={css:?}"
        );
        let a = compute_css_analytics(&css).expect("parses");
        assert_eq!(a.rule_count, 1);
    }

    #[test]
    fn stylex_create_is_atomic_one_bucket_per_key() {
        let src = "import * as stylex from '@stylexjs/stylex';\n\
                   export const styles = stylex.create({\n\
                   root: { color: 'red', padding: 16 },\n\
                   card: { color: 'blue' },\n\
                   });\n";
        let s = sheets(src);
        assert!(s.structural.is_none(), "stylex is atomic, not structural");
        let css = s.atomic.expect("stylex.create is atomic");
        assert!(css.contains("color:red;"), "css={css:?}");
        assert!(css.contains("padding:16px;"), "css={css:?}");
        assert!(css.contains("color:blue;"), "second bucket: css={css:?}");
        let a = compute_css_analytics(&css).expect("parses");
        assert!(a.rule_count >= 2, "two buckets: {a:?}");
    }

    #[test]
    fn panda_css_from_styled_system_is_atomic() {
        let src = "import { css } from '../styled-system/css';\n\
                   const c = css({ display: 'flex', gap: 8 });\n";
        let s = sheets(src);
        let css = s.atomic.expect("panda css is atomic");
        assert!(css.contains("display:flex;"), "css={css:?}");
        assert!(css.contains("gap:8px;"), "css={css:?}");
    }

    #[test]
    fn emotion_css_and_styled_are_structural() {
        let src = "import { css } from '@emotion/react';\n\
                   import styled from '@emotion/styled';\n\
                   const a = css({ color: 'red' });\n\
                   const B = styled.div({ fontWeight: 700 });\n";
        let css = sheets(src).structural.expect("emotion is structural");
        assert!(css.contains("color:red;"), "css={css:?}");
        assert!(css.contains("font-weight:700;"), "styled.div: css={css:?}");
    }

    #[test]
    fn styled_call_form_is_lifted() {
        let src = "import styled from '@emotion/styled';\n\
                   const Primary = styled(Button)({ fontWeight: 700 });\n";
        let css = sheets(src)
            .structural
            .expect("styled(Component)({}) lifted");
        assert!(css.contains("font-weight:700;"), "css={css:?}");
    }

    #[test]
    fn dynamic_value_is_dropped_to_structural_partial() {
        let src = "import { style } from '@vanilla-extract/css';\n\
                   import { theme } from './theme';\n\
                   const x = style({ color: theme.primary, padding: 8, margin: 4, top: 1, left: 2 });\n";
        let s = sheets(src);
        // The dynamic `color` is dropped; the bucket has a dropped decl so it
        // lands in structural_partial (duplicate-fingerprint suppressed by the
        // engine), NOT the clean structural sheet.
        assert!(s.structural.is_none(), "bucket had a drop: {s:?}");
        let css = s.structural_partial.expect("partial");
        assert!(
            !css.contains("fallowinterp"),
            "no placeholder, value dropped: {css:?}"
        );
        assert!(
            !css.contains("primary"),
            "dynamic member not serialized: {css:?}"
        );
        assert!(css.contains("padding:8px;"), "static survives: {css:?}");
        let a = compute_css_analytics(&css).expect("must parse, not None");
        assert_eq!(a.important_declarations, 0, "no invented !important: {a:?}");
    }

    #[test]
    fn spread_and_computed_key_dropped() {
        let src = "import { style } from '@vanilla-extract/css';\n\
                   const base = {};\n\
                   const k = 'color';\n\
                   const x = style({ ...base, [k]: 'red', padding: 8, margin: 4, top: 1 });\n";
        let s = sheets(src);
        // Spread + computed key are drops -> structural_partial.
        let css = s.structural_partial.expect("partial");
        assert!(css.contains("padding:8px;"), "static survives: {css:?}");
    }

    #[test]
    fn cva_variants_map_is_not_serialized_as_css() {
        // `cva` from class-variance-authority is NOT a recognized CSS-in-JS
        // import, so it must not fire at all even though `cva` is a Panda name.
        let cva = "import { cva } from 'class-variance-authority';\n\
                   const button = cva('base', { variants: { size: { sm: 'text-sm' } } });\n";
        assert!(
            sheets(cva).is_empty(),
            "unrelated cva must not fire: {:?}",
            sheets(cva)
        );

        // Panda `cva` from styled-system: only `base` is CSS; `variants` (a config
        // map of class objects) must be dropped, never serialized as garbage.
        let panda = "import { cva } from '../styled-system/css';\n\
                     const button = cva({ base: { color: 'red', padding: 8, margin: 4, top: 1 }, variants: { size: { sm: { fontSize: 12 } } } });\n";
        let s = sheets(panda);
        let css = s.atomic.expect("panda cva base is atomic");
        assert!(css.contains("color:red;"), "base serialized: {css:?}");
        assert!(
            !css.contains("size"),
            "variants config not serialized: {css:?}"
        );
        let a = compute_css_analytics(&css).expect("parses cleanly");
        assert!(
            a.notable_rules.is_empty(),
            "no garbled structural finding: {a:?}"
        );
    }

    #[test]
    fn panda_cva_and_class_variance_authority_cva_coexist() {
        // Both `cva` names can appear in one file only under distinct local
        // aliases (duplicate bindings are a JS error). class-variance-authority is
        // filtered out before the import map, so only Panda's binding is tracked:
        // its `base` lifts to atomic CSS while the class-name builder stays inert.
        let src = "import { cva } from '../styled-system/css';\n\
                   import { cva as cn } from 'class-variance-authority';\n\
                   const a = cva({ base: { color: 'red' } });\n\
                   const b = cn('base', { variants: { size: { sm: 'text-sm' } } });\n";
        let css = sheets(src).atomic.expect("panda cva base is atomic");
        assert!(css.contains("color:red;"), "panda base lifted: {css:?}");
        assert!(!css.contains("text-sm"), "cva-lib not serialized: {css:?}");
    }

    #[test]
    fn local_helper_with_recognized_name_does_not_fire() {
        // A local `const css = ...` with no recognized import must never fire,
        // even though `css` is a recognized library call name.
        let src = "const css = (o) => o;\n\
                   const x = css({ color: 'red', padding: 8 });\n";
        assert!(
            sheets(src).is_empty(),
            "local css helper must not fire: {:?}",
            sheets(src)
        );
    }

    #[test]
    fn type_only_import_does_not_open_the_gate() {
        let src = "import type { style } from '@vanilla-extract/css';\n\
                   const x = style({ color: 'red' });\n";
        assert!(
            sheets(src).is_empty(),
            "type-only import must not open provenance: {:?}",
            sheets(src)
        );
    }

    #[test]
    fn all_dynamic_bucket_emits_no_empty_rule() {
        let src = "import { style } from '@vanilla-extract/css';\n\
                   import { v } from './v';\n\
                   const x = style({ color: v.a, background: v.b });\n";
        let s = sheets(src);
        // Every value dynamic -> body empty -> bucket dropped entirely, no empty
        // `.fallow-css-in-js{}` rule in any sheet.
        assert!(s.is_empty(), "all-dynamic bucket dropped entirely: {s:?}");
    }

    #[test]
    fn aliased_named_import_still_recognized() {
        // `import { style as s }` dispatches on the canonical name, not the alias.
        let src = "import { style as s, globalStyle as gs } from '@vanilla-extract/css';\n\
                   export const a = s({ color: 'red' });\n\
                   gs('html', { margin: 0 });\n";
        let s = sheets(src);
        let css = s.structural.expect("aliased style/globalStyle recognized");
        assert!(css.contains("color:red;"), "aliased style fired: {css:?}");
        assert!(
            css.contains("html{margin:0;}"),
            "aliased globalStyle fired: {css:?}"
        );
    }

    #[test]
    fn emotion_css_default_import_recognized() {
        // `@emotion/css` default export IS the css function.
        let src = "import css from '@emotion/css';\n\
                   const a = css({ color: 'red' });\n";
        let css = sheets(src)
            .structural
            .expect("default css import recognized");
        assert!(css.contains("color:red;"), "css={css:?}");
    }

    #[test]
    fn emotion_css_default_import_aliased_recognized() {
        // The `@emotion/css` default css function fires under ANY local alias, not
        // only the conventional `css` name (canonical-role dispatch on the lib).
        let src = "import emo from '@emotion/css';\n\
                   const a = emo({ color: 'red' });\n";
        let css = sheets(src)
            .structural
            .expect("aliased default css import recognized");
        assert!(css.contains("color:red;"), "css={css:?}");
    }

    #[test]
    fn non_decimal_numeric_literals_become_valid_css() {
        // Hex / scientific literals render from their parsed value, never the raw
        // `0xFF` / `1e3` source text (which the CSS parser would reject).
        let src = "import { style } from '@vanilla-extract/css';\n\
                   const x = style({ padding: 0xFF, zIndex: 1e3 });\n";
        let css = sheets(src).structural.expect("structural");
        assert!(
            css.contains("padding:255px;"),
            "hex -> decimal px: css={css:?}"
        );
        assert!(
            css.contains("z-index:1000;"),
            "scientific -> decimal: css={css:?}"
        );
        assert!(compute_css_analytics(&css).is_some(), "valid CSS");
    }

    #[test]
    fn custom_property_numeric_value_keeps_no_unit() {
        // A numeric custom-property value must NOT gain an implicit `px`: emotion
        // (`!isCustomProperty`) and React both leave `--x: 8` unitless, so adding
        // `px` would fabricate a unit the bundler never emits.
        let src = "import { css } from '@emotion/react';\n\
                   const g = css({ ':root': { '--space': 8, '--ratio': 1.5 }, padding: 8 });\n";
        // `:root` is a selector key -> nested rule with the custom properties.
        let sheet = sheets(src)
            .structural
            .or_else(|| sheets(src).structural_partial)
            .expect("structural output");
        assert!(
            sheet.contains("--space:8;"),
            "custom prop keeps no unit: {sheet:?}"
        );
        assert!(
            sheet.contains("--ratio:1.5;"),
            "custom prop float unchanged: {sheet:?}"
        );
        // A normal property still gets px.
        assert!(
            sheet.contains("padding:8px;"),
            "normal prop still px: {sheet:?}"
        );
    }

    #[test]
    fn ms_vendor_prefix_kebabs_with_leading_dash() {
        assert_eq!(kebab_case("msFlexAlign"), "-ms-flex-align");
        assert_eq!(kebab_case("WebkitBoxShadow"), "-webkit-box-shadow");
        assert_eq!(kebab_case("backgroundColor"), "background-color");
        // `msg`-prefixed non-vendor names are not mangled.
        assert_eq!(kebab_case("msgType"), "msg-type");
    }

    #[test]
    fn negative_numbers_handled() {
        let src = "import { style } from '@vanilla-extract/css';\n\
                   const x = style({ marginTop: -8, zIndex: -1 });\n";
        let css = sheets(src).structural.expect("structural");
        assert!(css.contains("margin-top:-8px;"), "css={css:?}");
        assert!(
            css.contains("z-index:-1;"),
            "unitless negative: css={css:?}"
        );
    }

    #[test]
    fn none_without_any_object_css_in_js() {
        assert!(sheets("const x = 1; function f() {}").is_empty());
        assert!(sheets("import React from 'react'; const x = <div/>;").is_empty());
    }

    #[test]
    fn line_numbers_map_back_to_source() {
        // The `color` declaration's bucket is the `style({...})` object starting on
        // source line 3; the lifted sheet must keep a non-blank token at line 3.
        let src = "import { style } from '@vanilla-extract/css';\n\
                   \n\
                   const a = style({\n\
                   color: 'red',\n\
                   });\n";
        let css = sheets(src).structural.expect("structural");
        let pos = css.find("color").expect("color present");
        let css_line = 1 + css[..pos].bytes().filter(|&b| b == b'\n').count();
        assert_eq!(
            css_line, 3,
            "bucket maps to the style() object line: css={css:?}"
        );
    }

    #[test]
    fn multibyte_content_value_preserved() {
        let src = "import { style } from '@vanilla-extract/css';\n\
                   const x = style({ content: '\"café 日本 €\"', fontFamily: '\"Ñoño\"' });\n";
        let css = sheets(src).structural.expect("structural");
        assert!(
            css.contains("café 日本 €"),
            "multibyte preserved: css={css:?}"
        );
        assert!(
            compute_css_analytics(&css).is_some(),
            "valid UTF-8 / parses"
        );
    }

    #[test]
    fn distinct_colors_fall_out_of_object_styles() {
        let src = "import * as stylex from '@stylexjs/stylex';\n\
                   const s = stylex.create({ a: { color: 'red' }, b: { color: 'blue' }, c: { color: 'red' } });\n";
        let css = sheets(src).atomic.expect("atomic");
        let a = compute_css_analytics(&css).expect("parses");
        assert_eq!(a.colors.len(), 2, "distinct colors counted: {:?}", a.colors);
    }

    #[test]
    fn multi_bucket_padding_uses_key_line() {
        // Each stylex.create bucket pads to its KEY line, so two buckets do not
        // collapse onto the call line.
        let src = "import * as stylex from '@stylexjs/stylex';\n\
                   const s = stylex.create({\n\
                   root: { color: 'red' },\n\
                   card: { color: 'blue' },\n\
                   });\n";
        let css = sheets(src).atomic.expect("atomic");
        let red = css.find("color:red").expect("root present");
        let blue = css.find("color:blue").expect("card present");
        let red_line = 1 + css[..red].bytes().filter(|&b| b == b'\n').count();
        let blue_line = 1 + css[..blue].bytes().filter(|&b| b == b'\n').count();
        assert_eq!(red_line, 3, "root on its key line: css={css:?}");
        assert_eq!(blue_line, 4, "card on its own key line: css={css:?}");
    }
}
