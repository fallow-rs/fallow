//! Entry probes and the outline of a source file.
//!
//! A dry run that selects test files by reach records which functions each
//! test file enters. The instrumented text then calls the prelude function
//! [`PROBE_FUNCTION`] at the start of the module and at the start of the body
//! of each outer function, with a probe id. The runtime records a probe id
//! like a mutant hit.
//!
//! An outer function is a function that no other function contains: a
//! declaration, an expression, an arrow, a method, a getter, a setter or a
//! constructor. The body of a function is its block, or the expression of an
//! arrow. A function inside a body runs only after its outer function
//! entered, so the probe of the outer function covers it.
//!
//! The [`Outline`] of a text gives a hash of each outer function body and a
//! hash of the skeleton: the text with each outer function body replaced by
//! a marker. The skeleton holds the imports, the top-level statements, the
//! class field initializers, the static blocks, the decorators, and the name
//! and parameters of each function. Parameters are not in the body, because
//! code can read `fn.length` of a function that never runs. When the
//! skeleton and each body that a test file entered are equal to a recorded
//! outline, the test file runs the same code as in the recorded run, until it
//! enters a body that changed.

use oxc_ast::ast::{ArrowFunctionBody, ArrowFunctionExpression, Function, FunctionBody, Program};
use oxc_ast_visit::{Visit, walk};
use oxc_span::GetSpan;
use oxc_syntax::scope::ScopeFlags;
use xxhash_rust::xxh3::xxh3_64;

pub use crate::outline::{Outline, OutlineBody};

/// The prelude function that records the entry of a function body or a
/// module.
///
/// The instrumented output calls `__fallow_m_r(id)` with a probe id: `r`,
/// the [`probe_key`] of the path, a dot, and the ordinal of the outer
/// function in source order, or `m` for the module. Mutant ids are hex
/// digits, so a probe id never equals one.
pub const PROBE_FUNCTION: &str = "__fallow_m_r";

/// The prelude part of the entry probes. It follows the other prelude
/// functions, and the module probe follows it.
pub const PROBE_PRELUDE: &str =
    " function __fallow_m_r(id) { const m = globalThis.__fallowMutation; if (m) m.c(id); }";

/// The probe key of a root-relative path: 16 lowercase hex digits.
pub fn probe_key(path: &str) -> String {
    format!("{:016x}", xxh3_64(path.as_bytes()))
}

/// The probe id of the module of `key`.
pub fn module_probe(key: &str) -> String {
    format!("r{key}.m")
}

/// The probe id of the outer function `ordinal` of `key`.
pub fn function_probe(key: &str, ordinal: usize) -> String {
    format!("r{key}.{ordinal}")
}

/// Where the probe of a body goes.
#[derive(Debug, Clone, Copy)]
enum ProbeSite {
    /// A statement at `offset`, after the directive prologue of the block.
    /// `after_directive` asks for a `;` first, as a directive can end with
    /// no semicolon.
    Statement { offset: u32, after_directive: bool },
    /// A comma expression around the expression of an arrow.
    Expression,
}

#[derive(Debug, Clone, Copy)]
pub struct Body {
    /// The start of the function node: its parameters and its name start
    /// here or later.
    pub start: u32,
    /// The span of the body.
    pub span: (u32, u32),
    site: ProbeSite,
}

#[derive(Default)]
struct Collector {
    bodies: Vec<Body>,
    depth: u32,
}

impl Collector {
    fn block(&mut self, start: u32, body: &FunctionBody<'_>) {
        let site = match body.directives.last() {
            Some(directive) => ProbeSite::Statement {
                offset: directive.span.end,
                after_directive: true,
            },
            None => ProbeSite::Statement {
                offset: body.span.start + 1,
                after_directive: false,
            },
        };
        self.bodies.push(Body {
            start,
            span: (body.span.start, body.span.end),
            site,
        });
    }
}

impl<'a> Visit<'a> for Collector {
    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        if self.depth == 0
            && let Some(body) = &it.body
        {
            self.block(it.span.start, body);
        }
        self.depth += 1;
        walk::walk_function(self, it, flags);
        self.depth -= 1;
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        if self.depth == 0 {
            match &it.body {
                ArrowFunctionBody::FunctionBody(body) => self.block(it.span.start, body),
                expression => {
                    let span = expression.span();
                    self.bodies.push(Body {
                        start: it.span.start,
                        span: (span.start, span.end),
                        site: ProbeSite::Expression,
                    });
                }
            }
        }
        self.depth += 1;
        walk::walk_arrow_function_expression(self, it);
        self.depth -= 1;
    }
}

pub fn collect(program: &Program<'_>) -> Vec<Body> {
    let mut collector = Collector::default();
    collector.visit_program(program);
    let mut bodies = collector.bodies;
    bodies.sort_by_key(|body| body.span.0);
    bodies
}

/// The outline of the parsed `program` of `source`.
pub fn outline_of(program: &Program<'_>, source: &str) -> Outline {
    crate::outline::outline(program, source, &collect(program))
}

/// One probe text for the emitter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    pub offset: u32,
    pub text: String,
    /// For the `)` that ends a comma expression: the index of the probe
    /// that opens it. The emitter writes both in the same branch.
    pub opener: Option<usize>,
}

fn guarded_call(id: &str) -> String {
    format!("typeof {PROBE_FUNCTION} === \"function\" && {PROBE_FUNCTION}(\"{id}\")")
}

/// The probes of the outer function bodies of `program`, sorted by offset,
/// and the outline.
pub fn probes(program: &Program<'_>, source: &str, key: &str) -> (Vec<Probe>, Outline) {
    let bodies = collect(program);
    let mut probes = Vec::with_capacity(bodies.len() + 1);
    for (ordinal, body) in bodies.iter().enumerate() {
        let call = guarded_call(&function_probe(key, ordinal));
        match body.site {
            ProbeSite::Statement {
                offset,
                after_directive,
            } => probes.push(Probe {
                offset,
                text: if after_directive {
                    format!(";{call};")
                } else {
                    format!("{call};")
                },
                opener: None,
            }),
            ProbeSite::Expression => {
                let opener = probes.len();
                probes.push(Probe {
                    offset: body.span.0,
                    text: format!("({call}, "),
                    opener: None,
                });
                probes.push(Probe {
                    offset: body.span.1,
                    text: ")".to_owned(),
                    opener: Some(opener),
                });
            }
        }
    }
    // Outer bodies never nest, so a stable sort by offset keeps the `)` of
    // one arrow before the start of a body at the same offset.
    let order: Vec<usize> = {
        let mut order: Vec<usize> = (0..probes.len()).collect();
        order.sort_by_key(|&index| (probes[index].offset, probes[index].opener.is_none()));
        order
    };
    let mut position = vec![0; probes.len()];
    for (new, &old) in order.iter().enumerate() {
        position[old] = new;
    }
    let sorted = order
        .iter()
        .map(|&old| {
            let probe = &probes[old];
            Probe {
                offset: probe.offset,
                text: probe.text.clone(),
                opener: probe.opener.map(|opener| position[opener]),
            }
        })
        .collect();
    (sorted, crate::outline::outline(program, source, &bodies))
}

/// The module probe statement, which follows the prelude.
pub fn module_statement(key: &str) -> String {
    format!(" {PROBE_FUNCTION}(\"{}\");", module_probe(key))
}
