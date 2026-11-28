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

#[derive(Default)]
struct References {
    names: Vec<(u32, String)>,
}

impl<'a> Visit<'a> for References {
    fn visit_identifier_reference(&mut self, it: &oxc_ast::ast::IdentifierReference<'a>) {
        self.names.push((it.span.start, it.name.to_string()));
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
        Statement::TSTypeAliasDeclaration(_) | Statement::TSInterfaceDeclaration(_) => Group::Type,
        Statement::TSExternalModuleDeclaration(module) if module.declare => Group::Type,
        Statement::TSNamespaceDeclaration(module) if module.declare => Group::Type,
        Statement::TSGlobalDeclaration(_) => Group::Type,
        Statement::ImportDeclaration(import) => {
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
                    ImportDeclarationSpecifier::ImportSpecifier(named) => {
                        (!named.import_kind.is_type()).then(|| {
                            (
                                named.local.name.to_string(),
                                named.imported.name().to_string(),
                            )
                        })
                    }
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
        Statement::FunctionDeclaration(function) => match (&function.id, &function.body) {
            _ if function.declare => Group::Type,
            (Some(id), Some(_)) => Group::Function {
                name: id.name.to_string(),
            },
            _ => Group::Code { name: None },
        },
        Statement::VariableDeclaration(declaration) => {
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
        Statement::ExportNamedDeclaration(export) if export.export_kind.is_type() => Group::Type,
        Statement::ExportFromDeclaration(export) if export.export_kind.is_type() => Group::Type,
        Statement::ExportDeclaration(export) => {
            let name = match &export.declaration {
                Declaration::TSTypeAliasDeclaration(_) | Declaration::TSInterfaceDeclaration(_) => {
                    return Group::Type;
                }
                Declaration::FunctionDeclaration(function) if function.declare => {
                    return Group::Type;
                }
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
        Statement::ExportDefaultDeclaration(_) => Group::Code {
            name: Some("default".to_owned()),
        },
        Statement::ClassDeclaration(class) => Group::Code {
            name: class.id.as_ref().map(|id| format!("class:{}", id.name)),
        },
        _ => Group::Code { name: None },
    }
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

/// Gives the outline of `program`, the parsed `source`, with its outer
/// function `bodies` in source order.
pub(crate) fn outline(program: &Program<'_>, source: &str, bodies: &[Body]) -> Outline {
    let masker = Masker {
        source,
        comments: program
            .comments
            .iter()
            .map(|comment| {
                let text = source
                    .get(comment.span.start as usize..comment.span.end as usize)
                    .unwrap_or_default();
                Mask {
                    span: (comment.span.start, comment.span.end),
                    with: if text.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
                        b"\n"
                    } else {
                        b" "
                    },
                }
            })
            .collect(),
    };
    let mut references = References::default();
    references.visit_program(program);
    let mut refs = references.names;
    refs.sort_unstable();
    let refs_in = |from: u32, to: u32, outside: &[(u32, u32)]| -> BTreeSet<String> {
        let start = refs.partition_point(|(offset, _)| *offset < from);
        refs[start..]
            .iter()
            .take_while(|(offset, _)| *offset < to)
            .filter(|(offset, _)| {
                !outside
                    .iter()
                    .any(|body| body.0 <= *offset && *offset < body.1)
            })
            .map(|(_, name)| name.clone())
            .collect()
    };
    let body_spans: Vec<(u32, u32)> = bodies.iter().map(|body| body.span).collect();
    let mut keys: Vec<Option<String>> = vec![None; bodies.len()];
    let mut skeleton = Xxh3::new();
    let mut skeleton_refs = BTreeSet::new();
    let mut bindings = BTreeMap::new();
    for statement in &program.body {
        let span = statement.span();
        let first = bodies.partition_point(|body| body.start < span.start);
        let last = bodies.partition_point(|body| body.start < span.end);
        let inside = &body_spans[first..last];
        match group(statement) {
            Group::Type => {}
            Group::Import {
                source: from,
                bindings: imported,
            } => {
                skeleton.update(b"import\0");
                skeleton.update(from.as_bytes());
                skeleton.update(b"\0");
                for (local, name) in imported {
                    let mut hasher = Xxh3::new();
                    hasher.update(from.as_bytes());
                    hasher.update(b"\0");
                    hasher.update(name.as_bytes());
                    bindings.insert(local, hasher.digest());
                }
            }
            Group::Function { name } => {
                for (index, key) in (first..last).zip(&mut keys[first..last]) {
                    *key = Some(if index == first {
                        format!("fn:{name}")
                    } else {
                        format!("fn:{name}#{}", index - first)
                    });
                }
                // The binding is the name and the parameters. The body
                // counts only for a test file that entered it.
                let header = bodies
                    .get(first)
                    .map_or((span.start, span.end), |body| (span.start, body.span.0));
                bindings.insert(name, masker.digest(header, &[]));
            }
            Group::Variables { kind, declarators } => {
                for (name, declarator) in declarators {
                    let from = bodies.partition_point(|body| body.start < declarator.0);
                    let to = bodies.partition_point(|body| body.start < declarator.1);
                    for (index, key) in (from..to).zip(&mut keys[from..to]) {
                        *key = Some(format!("var:{name}#{}", index - from));
                    }
                    let mut hasher = Xxh3::new();
                    hasher.update(kind.as_bytes());
                    hasher.update(b"\0");
                    masker.hash(declarator, &body_spans[from..to], &mut hasher);
                    bindings.insert(name, hasher.digest());
                }
            }
            Group::Code { name } => {
                let digest = masker.digest((span.start, span.end), inside);
                skeleton.update(b"code\0");
                skeleton.update(&digest.to_le_bytes());
                skeleton_refs.extend(refs_in(span.start, span.end, inside));
                let prefix = name.unwrap_or_else(|| format!("s{digest:016x}"));
                for (index, key) in (first..last).zip(&mut keys[first..last]) {
                    *key = Some(format!("{prefix}#{}", index - first));
                }
            }
        }
    }
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let outline_bodies = bodies
        .iter()
        .zip(keys)
        .enumerate()
        .map(|(index, (body, key))| {
            let base = key.unwrap_or_else(|| format!("at{index}"));
            let count = seen.entry(base.clone()).or_default();
            let key = if *count == 0 {
                base.clone()
            } else {
                format!("{base}~{count}")
            };
            *count += 1;
            OutlineBody {
                span: body.span,
                hash: masker.digest((body.start, body.span.1), &[]),
                key,
                refs: refs_in(body.start, body.span.1, &[]).into_iter().collect(),
            }
        })
        .collect();
    let all_refs: BTreeSet<String> = refs.into_iter().map(|(_, name)| name).collect();
    Outline {
        skeleton: skeleton.digest(),
        bodies: outline_bodies,
        bindings,
        skeleton_refs: skeleton_refs.into_iter().collect(),
        refs: all_refs.into_iter().collect(),
    }
}
