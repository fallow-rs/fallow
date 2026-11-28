//! The outline of a source text: the parts that the reach record compares
//! between two versions of a file.
//!
//! The top-level statements fall into four groups:
//!
//! - Type-only statements (interfaces, type aliases, `import type`,
//!   `export type`, `declare` statements). The runtime never sees them, so
//!   the outline leaves them out.
//! - Imports. The skeleton holds the source of each import in order, because
//!   the order of the sources is the order in which the modules load. Each
//!   imported binding is a binding of the outline.
//! - Pure declarations: a function declaration with no `export`, and a
//!   `const`, `let` or `var` with no `export` whose initializers run no code
//!   (literals, arrays and objects of them, function expressions and arrows).
//!   Each declared name is a binding: a function declaration gives the hash
//!   of its name and parameters, a variable the hash of its declarator with
//!   each function body left out.
//! - Each other statement is module code: the skeleton holds its text with
//!   each outer function body replaced by a marker.
//!
//! Comments do not count. An outer function gets a key that stays the same
//! when other code moves: the name of its declaration, or the hash of the
//! statement that holds it and its index there. Each outer function also
//! lists the names that its text references, so a change of a binding keeps
//! only the test files that entered a function that names it.

use std::collections::{BTreeMap, BTreeSet};

use oxc_ast::ast::{
    ArrayExpressionElement, Declaration, Expression, ImportDeclarationSpecifier,
    ObjectPropertyKind, Program, Statement, UnaryOperator,
};
use oxc_ast_visit::Visit;
use oxc_span::GetSpan;
use serde::{Deserialize, Serialize};
use xxhash_rust::xxh3::Xxh3;

use crate::reach::Body;

/// One outer function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutlineBody {
    /// The byte span of the body: the block with its braces, or the
    /// expression of an arrow.
    pub span: (u32, u32),
    /// The hash of the function text: its name, parameters and body.
    pub hash: u64,
    /// The key of the function. Another version of the file gives the same
    /// function the same key.
    pub key: String,
    /// The names that the function text references, sorted.
    pub refs: Vec<String>,
}

/// The outline of a source text. See the module docs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outline {
    /// The hash of the module code and of the import sources.
    pub skeleton: u64,
    /// The outer functions, in source order. The index is the ordinal of the
    /// probe id.
    pub bodies: Vec<OutlineBody>,
    /// The hash of each binding of an import or a pure declaration.
    pub bindings: BTreeMap<String, u64>,
    /// The names that the module code references outside each function
    /// body, sorted.
    pub skeleton_refs: Vec<String>,
    /// Each name that the text references, sorted.
    pub refs: Vec<String>,
    /// The span of each declarator of a pure variable declaration, with its
    /// name, in source order. A mutant in such a declarator runs when the
    /// module loads, and only code that reads the name can observe it.
    #[serde(default)]
    pub declarators: Vec<(String, (u32, u32))>,
}

impl Outline {
    /// The ordinal of the outer function body that holds `span`, or `None`
    /// when the span is outside each function body.
    pub fn body_of(&self, span: (u32, u32)) -> Option<usize> {
        let index = self
            .bodies
            .partition_point(|body| body.span.0 <= span.0)
            .checked_sub(1)?;
        let body = self.bodies.get(index)?;
        (span.1 <= body.span.1).then_some(index)
    }

    /// The name of the pure variable declarator that holds `span`.
    pub fn declarator_of(&self, span: (u32, u32)) -> Option<&str> {
        self.declarators
            .iter()
            .find(|(_, declarator)| declarator.0 <= span.0 && span.1 <= declarator.1)
            .map(|(name, _)| name.as_str())
    }

    /// The ordinal of the function with `key`.
    pub fn function(&self, key: &str) -> Option<usize> {
        self.bodies.iter().position(|body| body.key == key)
    }
}

/// The marker of a left-out function body. No source text has the byte.
const BODY_MARKER: &[u8] = &[0xff];

/// A part of the text that a hash leaves out: a comment (replaced by a
/// space, or by a line break when it holds one, as the line break counts for
/// automatic semicolon insertion) or a function body (replaced by
/// [`BODY_MARKER`]).
#[derive(Debug, Clone, Copy)]
struct Mask {
    span: (u32, u32),
    with: &'static [u8],
}

struct Masker<'s> {
    source: &'s str,
    comments: Vec<Mask>,
}

impl Masker<'_> {
    /// The hash of `range` with each comment and each of `bodies` left out.
    fn hash(&self, range: (u32, u32), bodies: &[(u32, u32)], hasher: &mut Xxh3) {
        let mut masks: Vec<Mask> = bodies
            .iter()
            .map(|&span| Mask {
                span,
                with: BODY_MARKER,
            })
            .collect();
        masks.extend(self.comments.iter().copied().filter(|comment| {
            !bodies
                .iter()
                .any(|body| body.0 <= comment.span.0 && comment.span.1 <= body.1)
        }));
        masks.retain(|mask| range.0 <= mask.span.0 && mask.span.1 <= range.1);
        masks.sort_by_key(|mask| mask.span.0);
        let mut cursor = range.0;
        for mask in masks {
            if mask.span.0 < cursor {
                continue;
            }
            hasher.update(self.text(cursor, mask.span.0).as_bytes());
            hasher.update(mask.with);
            cursor = mask.span.1;
        }
        hasher.update(self.text(cursor, range.1).as_bytes());
    }

    fn digest(&self, range: (u32, u32), bodies: &[(u32, u32)]) -> u64 {
        let mut hasher = Xxh3::new();
        self.hash(range, bodies, &mut hasher);
        hasher.digest()
    }

    fn text(&self, from: u32, to: u32) -> &str {
        self.source
            .get(from as usize..to as usize)
            .unwrap_or_default()
    }
}

/// The names that the text references. A name in a TypeScript type is not a
/// value reference: the transform removes it, and an import whose names have
/// only such references does not load its module.
#[derive(Default)]
struct References {
    /// The offset, the name, and true for a value reference.
    names: Vec<(u32, String, bool)>,
    types: u32,
}

impl<'a> Visit<'a> for References {
    fn visit_identifier_reference(&mut self, it: &oxc_ast::ast::IdentifierReference<'a>) {
        self.names
            .push((it.span.start, it.name.to_string(), self.types == 0));
    }

    fn visit_ts_type(&mut self, it: &oxc_ast::ast::TSType<'a>) {
        self.types += 1;
        oxc_ast_visit::walk::walk_ts_type(self, it);
        self.types -= 1;
    }
}

/// True when evaluating `expression` runs no code.
fn is_pure(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::RegExpLiteral(_)
        | Expression::ArrowFunctionExpression(_)
        | Expression::FunctionExpression(_) => true,
        Expression::TemplateLiteral(template) => template.expressions.is_empty(),
        Expression::ArrayExpression(array) => array.elements.iter().all(|element| match element {
            ArrayExpressionElement::Elision(_) => true,
            ArrayExpressionElement::SpreadElement(_) => false,
            other => other.as_expression().is_some_and(is_pure),
        }),
        Expression::ObjectExpression(object) => {
            object.properties.iter().all(|property| match property {
                ObjectPropertyKind::ObjectProperty(property) => {
                    !property.computed && is_pure(&property.value)
                }
                ObjectPropertyKind::SpreadProperty(_) => false,
            })
        }
        Expression::UnaryExpression(unary) => {
            matches!(
                unary.operator,
                UnaryOperator::UnaryNegation | UnaryOperator::UnaryPlus | UnaryOperator::LogicalNot
            ) && is_pure(&unary.argument)
        }
        Expression::ParenthesizedExpression(inner) => is_pure(&inner.expression),
        Expression::TSAsExpression(inner) => is_pure(&inner.expression),
        Expression::TSSatisfiesExpression(inner) => is_pure(&inner.expression),
        Expression::TSNonNullExpression(inner) => is_pure(&inner.expression),
        Expression::TSTypeAssertion(inner) => is_pure(&inner.expression),
        _ => false,
    }
}

/// The group of one top-level statement. See the module docs.
enum Group {
    Type,
    Import {
        source: String,
        bindings: Vec<(String, String)>,
    },
    Function {
        name: String,
    },
    Variables {
        /// `const`, `let` or `var`.
        kind: &'static str,
        /// The name and the declarator span of each declarator.
        declarators: Vec<(String, (u32, u32))>,
    },
    /// Module code. The prefix of the key of its functions, when the
    /// statement names them: a declaration name or `default`.
    Code {
        name: Option<String>,
    },
}

fn group(statement: &Statement<'_>) -> Group {
    match statement {
        Statement::TSTypeAliasDeclaration(_)
        | Statement::TSInterfaceDeclaration(_)
        | Statement::TSGlobalDeclaration(_) => Group::Type,
        Statement::TSExternalModuleDeclaration(module) if module.declare => Group::Type,
        Statement::TSNamespaceDeclaration(module) if module.declare => Group::Type,
        Statement::ImportDeclaration(import) => import_group(import),
        Statement::FunctionDeclaration(function) => match (&function.id, &function.body) {
            _ if function.declare => Group::Type,
            (Some(id), Some(_)) => Group::Function {
                name: id.name.to_string(),
            },
            _ => Group::Code { name: None },
        },
        Statement::VariableDeclaration(declaration) => variables_group(declaration),
        Statement::ExportNamedDeclaration(export) if export.export_kind.is_type() => Group::Type,
        Statement::ExportFromDeclaration(export) if export.export_kind.is_type() => Group::Type,
        Statement::ExportDeclaration(export) => export_group(&export.declaration),
        Statement::ExportDefaultDeclaration(_) => Group::Code {
            name: Some("default".to_owned()),
        },
        Statement::ClassDeclaration(class) => Group::Code {
            name: class.id.as_ref().map(|id| format!("class:{}", id.name)),
        },
        _ => Group::Code { name: None },
    }
}

fn import_group(import: &oxc_ast::ast::ImportDeclaration<'_>) -> Group {
    if import.import_kind.is_type() {
        return Group::Type;
    }
    let Some(specifiers) = &import.specifiers else {
        // A side-effect import runs the module.
        return Group::Code { name: None };
    };
    let source = import.source.value.as_str().unwrap_or_default().to_owned();
    let bindings = specifiers
        .iter()
        .filter_map(|specifier| match specifier {
            ImportDeclarationSpecifier::ImportSpecifier(named) => (!named.import_kind.is_type())
                .then(|| {
                    (
                        named.local.name.to_string(),
                        named.imported.name().to_string(),
                    )
                }),
            ImportDeclarationSpecifier::ImportDefaultSpecifier(default) => {
                Some((default.local.name.to_string(), "default".to_owned()))
            }
            ImportDeclarationSpecifier::ImportNamespaceSpecifier(namespace) => {
                Some((namespace.local.name.to_string(), "*".to_owned()))
            }
        })
        .collect();
    Group::Import { source, bindings }
}

fn variables_group(declaration: &oxc_ast::ast::VariableDeclaration<'_>) -> Group {
    if declaration.declare {
        return Group::Type;
    }
    let declarators: Option<Vec<(String, (u32, u32))>> = declaration
        .declarations
        .iter()
        .map(|declarator| {
            let id = declarator.id.get_binding_identifier()?;
            let pure = declarator.init.as_ref().is_none_or(is_pure);
            pure.then(|| {
                (
                    id.name.to_string(),
                    (declarator.span.start, declarator.span.end),
                )
            })
        })
        .collect();
    match declarators {
        Some(declarators) => Group::Variables {
            kind: declaration.kind.as_str(),
            declarators,
        },
        None => Group::Code {
            name: single_variable(declaration),
        },
    }
}

fn export_group(declaration: &Declaration<'_>) -> Group {
    let name = match declaration {
        Declaration::TSTypeAliasDeclaration(_) | Declaration::TSInterfaceDeclaration(_) => {
            return Group::Type;
        }
        Declaration::FunctionDeclaration(function) if function.declare => return Group::Type,
        Declaration::VariableDeclaration(declaration) if declaration.declare => {
            return Group::Type;
        }
        Declaration::FunctionDeclaration(function) => {
            function.id.as_ref().map(|id| format!("fn:{}", id.name))
        }
        Declaration::ClassDeclaration(class) => {
            class.id.as_ref().map(|id| format!("class:{}", id.name))
        }
        Declaration::VariableDeclaration(declaration) => single_variable(declaration),
        _ => None,
    };
    Group::Code { name }
}

fn single_variable(declaration: &oxc_ast::ast::VariableDeclaration<'_>) -> Option<String> {
    match declaration.declarations.as_slice() {
        [declarator] => declarator
            .id
            .get_binding_identifier()
            .map(|id| format!("var:{}", id.name)),
        _ => None,
    }
}

/// True for a comment that a transform reads, such as `@jsx` or
/// `@jsxImportSource`. Such a comment changes the code of the module.
fn is_pragma(text: &str) -> bool {
    text.contains("@jsx")
}

fn masker<'s>(program: &Program<'_>, source: &'s str) -> Masker<'s> {
    Masker {
        source,
        comments: program
            .comments
            .iter()
            .filter_map(|comment| {
                let text = source
                    .get(comment.span.start as usize..comment.span.end as usize)
                    .unwrap_or_default();
                (!is_pragma(text)).then_some(Mask {
                    span: (comment.span.start, comment.span.end),
                    with: if text.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
                        b"\n"
                    } else {
                        b" "
                    },
                })
            })
            .collect(),
    }
}

/// The state of [`outline`] while it walks the top-level statements.
struct Builder<'s, 'b> {
    masker: Masker<'s>,
    bodies: &'b [Body],
    body_spans: Vec<(u32, u32)>,
    /// The references, sorted by offset.
    refs: Vec<(u32, String, bool)>,
    /// The imported names that the file reads as values. The binding
    /// resolution of the semantic pass decides it, so a local of the same
    /// name in an inner scope does not count.
    value_imports: BTreeSet<String>,
    keys: Vec<Option<String>>,
    skeleton: Xxh3,
    skeleton_refs: BTreeSet<String>,
    bindings: BTreeMap<String, u64>,
    declarators: Vec<(String, (u32, u32))>,
}

impl Builder<'_, '_> {
    /// The names that `from..to` references, outside the `outside` spans.
    fn refs_in(&self, from: u32, to: u32, outside: &[(u32, u32)]) -> BTreeSet<String> {
        let start = self.refs.partition_point(|(offset, ..)| *offset < from);
        self.refs[start..]
            .iter()
            .take_while(|(offset, ..)| *offset < to)
            .filter(|(offset, ..)| {
                !outside
                    .iter()
                    .any(|body| body.0 <= *offset && *offset < body.1)
            })
            .map(|(_, name, _)| name.clone())
            .collect()
    }

    /// The range of the outer functions that start in `span`.
    fn functions_in(&self, span: (u32, u32)) -> std::ops::Range<usize> {
        let first = self.bodies.partition_point(|body| body.start < span.0);
        let last = self.bodies.partition_point(|body| body.start < span.1);
        first..last
    }

    fn name_functions(&mut self, range: std::ops::Range<usize>, key: impl Fn(usize) -> String) {
        let first = range.start;
        for index in range {
            self.keys[index] = Some(key(index - first));
        }
    }

    fn statement(&mut self, statement: &Statement<'_>) {
        let span = statement.span();
        let span = (span.start, span.end);
        let functions = self.functions_in(span);
        match group(statement) {
            Group::Type => {}
            Group::Import { source, bindings } => self.import(&source, bindings),
            Group::Function { name } => {
                self.name_functions(functions.clone(), |index| {
                    if index == 0 {
                        format!("fn:{name}")
                    } else {
                        format!("fn:{name}#{index}")
                    }
                });
                // The binding is the name and the parameters. The body counts
                // only for a test file that entered it.
                let header = self
                    .bodies
                    .get(functions.start)
                    .map_or(span, |body| (span.0, body.span.0));
                let digest = self.masker.digest(header, &[]);
                self.bindings.insert(name, digest);
            }
            Group::Variables { kind, declarators } => {
                for (name, declarator) in declarators {
                    let inner = self.functions_in(declarator);
                    self.name_functions(inner.clone(), |index| format!("var:{name}#{index}"));
                    let mut hasher = Xxh3::new();
                    hasher.update(kind.as_bytes());
                    hasher.update(b"\0");
                    self.masker
                        .hash(declarator, &self.body_spans[inner], &mut hasher);
                    self.bindings.insert(name.clone(), hasher.digest());
                    self.declarators.push((name, declarator));
                }
            }
            Group::Code { name } => {
                let inside = &self.body_spans[functions.clone()];
                let digest = self.masker.digest(span, inside);
                self.skeleton.update(b"code\0");
                self.skeleton.update(&digest.to_le_bytes());
                let refs = self.refs_in(span.0, span.1, inside);
                self.skeleton_refs.extend(refs);
                let prefix = name.unwrap_or_else(|| format!("s{digest:016x}"));
                self.name_functions(functions, |index| format!("{prefix}#{index}"));
            }
        }
    }

    /// An import is module code by its source and by whether the file reads
    /// one of its names as a value: a TypeScript transform removes an import
    /// with no such read, and then the module does not load.
    fn import(&mut self, source: &str, bindings: Vec<(String, String)>) {
        self.skeleton.update(b"import\0");
        self.skeleton.update(source.as_bytes());
        let loads = bindings
            .iter()
            .any(|(local, _)| self.value_imports.contains(local));
        self.skeleton
            .update(if loads { b"\0load" } else { b"\0type" });
        for (local, name) in bindings {
            let mut hasher = Xxh3::new();
            hasher.update(source.as_bytes());
            hasher.update(b"\0");
            hasher.update(name.as_bytes());
            self.bindings.insert(local, hasher.digest());
        }
        self.skeleton.update(b"\0");
    }

    fn finish(self) -> Outline {
        let mut seen: BTreeMap<String, usize> = BTreeMap::new();
        let bodies = self
            .bodies
            .iter()
            .zip(&self.keys)
            .enumerate()
            .map(|(index, (body, key))| {
                let base = key.clone().unwrap_or_else(|| format!("at{index}"));
                let count = seen.entry(base.clone()).or_default();
                let key = if *count == 0 {
                    base
                } else {
                    format!("{base}~{count}")
                };
                *count += 1;
                OutlineBody {
                    span: body.span,
                    hash: self.masker.digest((body.start, body.span.1), &[]),
                    key,
                    refs: self
                        .refs_in(body.start, body.span.1, &[])
                        .into_iter()
                        .collect(),
                }
            })
            .collect();
        let refs: BTreeSet<String> = self.refs.into_iter().map(|(_, name, _)| name).collect();
        Outline {
            skeleton: self.skeleton.digest(),
            bodies,
            bindings: self.bindings,
            skeleton_refs: self.skeleton_refs.into_iter().collect(),
            refs: refs.into_iter().collect(),
            declarators: self.declarators,
        }
    }
}

/// The local names of the imports that the program reads as values. A
/// TypeScript transform removes an import binding with no such read.
fn value_imports(program: &Program<'_>) -> BTreeSet<String> {
    let semantic = oxc_semantic::SemanticBuilder::new().build(program).semantic;
    let scoping = semantic.scoping();
    program
        .body
        .iter()
        .filter_map(|statement| match statement {
            Statement::ImportDeclaration(import) => import.specifiers.as_ref(),
            _ => None,
        })
        .flatten()
        .filter(|specifier| {
            scoping
                .get_resolved_references(specifier.symbol_id())
                .any(|reference| reference.is_value() && !reference.flags().is_value_as_type())
        })
        .map(|specifier| specifier.local().name.to_string())
        .collect()
}

/// Gives the outline of `program`, the parsed `source`, with its outer
/// function `bodies` in source order.
pub fn outline(program: &Program<'_>, source: &str, bodies: &[Body]) -> Outline {
    let mut references = References::default();
    references.visit_program(program);
    let mut refs = references.names;
    refs.sort_unstable();
    let value_imports = value_imports(program);
    let masker = masker(program, source);
    let mut skeleton = Xxh3::new();
    // A directive such as "use strict" changes how each function runs, and
    // a pragma comment changes the code that the transform gives.
    for directive in &program.directives {
        skeleton.update(b"directive\0");
        skeleton.update(
            masker
                .text(directive.span.start, directive.span.end)
                .as_bytes(),
        );
    }
    for comment in &program.comments {
        let text = masker.text(comment.span.start, comment.span.end);
        if is_pragma(text) {
            skeleton.update(b"pragma\0");
            skeleton.update(text.as_bytes());
        }
    }
    let mut builder = Builder {
        masker,
        bodies,
        body_spans: bodies.iter().map(|body| body.span).collect(),
        refs,
        value_imports,
        keys: vec![None; bodies.len()],
        skeleton,
        skeleton_refs: BTreeSet::new(),
        bindings: BTreeMap::new(),
        declarators: Vec::new(),
    };
    for statement in &program.body {
        builder.statement(statement);
    }
    builder.finish()
}
