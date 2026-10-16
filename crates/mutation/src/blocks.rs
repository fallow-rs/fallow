//! Instrumentation of the script blocks in a component file.
//!
//! A Vue or Svelte single-file component and an Astro component hold their
//! code in script blocks. The instrumented file is the whole component file
//! with each script block replaced by its instrumented text, so the framework
//! compiler sees the same template and styles. The caller finds the blocks;
//! this crate has no component parser.
//!
//! The framework compilers move some code out of the block scope. A mutation
//! switch in that code cannot reach the prelude, or the compiler refuses the
//! switch. These positions give no mutant:
//!
//! - Vue `<script setup>`: a compiler macro call and its arguments
//!   (`defineProps`, `defineExpose`, `withDefaults` and the other `define*`
//!   macros), and the destructure defaults of a declaration with a macro
//!   call. The compiler moves them out of `setup()` or replaces the call, so
//!   a switch around the call hides the macro.
//! - Svelte: an expression that holds a rune call (`$state(...)`,
//!   `$effect(...)` and the other runes). A rune must stay in its own
//!   position. The rune arguments still give mutants.
//! - Astro frontmatter: an exported declaration. The compiler moves it out of
//!   the render function.

use std::collections::BTreeSet;

use oxc_ast::ast::{CallExpression, Expression, Program, Statement, VariableDeclarator};
use oxc_ast_visit::{Visit, walk};
use oxc_span::GetSpan;

use crate::{
    Instrumented, MutantId, MutationError, MutationOptions, SourceKind, SpliceMap, SpliceSegment,
    TextInput, instrument_text, line_column, line_starts,
};

/// The role of a script block in a component file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptRole {
    /// A Vue `<script>` block without `setup`. The code runs at module scope.
    VueScript,
    /// A Vue `<script setup>` block. The code runs in `setup()`.
    VueSetup,
    /// A Svelte module script (`<script module>` or
    /// `<script context="module">`). The code runs at module scope.
    SvelteModule,
    /// A Svelte instance script. The code runs once for each component
    /// instance.
    SvelteInstance,
    /// The frontmatter of an Astro component. The code runs in the render
    /// function.
    AstroFrontmatter,
}

impl ScriptRole {
    /// True when the block code runs at module scope.
    const fn is_module_scope(self) -> bool {
        matches!(self, Self::VueScript | Self::SvelteModule)
    }
}

/// One script block of a component file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptBlock {
    /// The byte span of the block body in the component file.
    pub span: (u32, u32),
    /// The language of the block body.
    pub kind: SourceKind,
    /// The role of the block.
    pub role: ScriptRole,
}

/// Finds the mutants in the script blocks of a component file and returns
/// the instrumented file.
///
/// The spans, lines and columns of the mutants, the ranges of `options` and
/// the splice map use the coordinates of the whole file. The text outside
/// the blocks stays the same. A block that is outside the file, or that
/// overlaps an earlier block, is ignored.
///
/// The prelude goes into each block with module scope. When the file has no
/// such block, it goes into each block, because each block then has its own
/// scope.
pub fn instrument_blocks(
    source: &str,
    path: &str,
    blocks: &[ScriptBlock],
    options: &MutationOptions,
) -> Result<Instrumented, MutationError> {
    if u32::try_from(source.len()).is_err() {
        return Err(MutationError::TooLarge);
    }
    let ordered = valid_blocks(source, blocks);
    let module_scope = ordered.iter().any(|block| block.role.is_module_scope());
    let starts = line_starts(source);
    let mut seen_ids: BTreeSet<MutantId> = BTreeSet::new();
    let mut output = Instrumented {
        mutants: Vec::new(),
        code: String::with_capacity(source.len() * 2),
        map: SpliceMap::default(),
    };
    let mut cursor = 0;
    for block in ordered {
        copy_original(source, (cursor, block.span.0), &mut output);
        let (start, end) = block.span;
        let input = TextInput {
            source: &source[start as usize..end as usize],
            path,
            kind: block.kind,
            role: Some(block.role),
            prelude: !module_scope || block.role.is_module_scope(),
        };
        let local_options = MutationOptions {
            operators: options.operators.clone(),
            ranges: options
                .ranges
                .as_ref()
                .map(|ranges| local_ranges(ranges, block.span)),
        };
        let result = instrument_text(&input, &local_options, &mut seen_ids)?;
        append_block(result, start, &starts, &mut output)?;
        cursor = end;
    }
    copy_original(source, (cursor, source.len() as u32), &mut output);
    output.mutants.sort_by_key(|mutant| mutant.span);
    if u32::try_from(output.code.len()).is_err() {
        return Err(MutationError::TooLarge);
    }
    Ok(output)
}

/// The blocks in file order, without a block that is outside the file, that
/// does not start and end on a character boundary, or that overlaps an
/// earlier block.
fn valid_blocks(source: &str, blocks: &[ScriptBlock]) -> Vec<ScriptBlock> {
    let mut ordered: Vec<ScriptBlock> = blocks
        .iter()
        .copied()
        .filter(|block| {
            let (start, end) = (block.span.0 as usize, block.span.1 as usize);
            start <= end && source.get(start..end).is_some()
        })
        .collect();
    ordered.sort_by_key(|block| block.span);
    let mut end = 0;
    ordered.retain(|block| {
        let keep = block.span.0 >= end;
        if keep {
            end = block.span.1;
        }
        keep
    });
    ordered
}

/// The parts of `ranges` inside `block`, relative to the block start.
fn local_ranges(ranges: &[(u32, u32)], block: (u32, u32)) -> Vec<(u32, u32)> {
    ranges
        .iter()
        .filter_map(|&(from, to)| {
            let start = from.max(block.0);
            let end = to.min(block.1);
            (start < end).then(|| (start - block.0, end - block.0))
        })
        .collect()
}

fn copy_original(source: &str, span: (u32, u32), output: &mut Instrumented) {
    let (from, to) = span;
    if to <= from {
        return;
    }
    output.map.segments.push(SpliceSegment {
        generated_start: output.code.len() as u32,
        original_start: from,
        len: to - from,
    });
    output.code.push_str(&source[from as usize..to as usize]);
}

/// Appends the instrumented block at the end of `output` and moves its
/// mutants and map to the coordinates of the whole file.
fn append_block(
    block: Instrumented,
    start: u32,
    starts: &[u32],
    output: &mut Instrumented,
) -> Result<(), MutationError> {
    let base = u32::try_from(output.code.len()).map_err(|_| MutationError::TooLarge)?;
    output
        .map
        .segments
        .extend(block.map.segments.into_iter().map(|segment| SpliceSegment {
            generated_start: segment.generated_start + base,
            original_start: segment.original_start + start,
            len: segment.len,
        }));
    output.code.push_str(&block.code);
    for mut mutant in block.mutants {
        mutant.span = (mutant.span.0 + start, mutant.span.1 + start);
        (mutant.line, mutant.column) = line_column(starts, mutant.span.0);
        if let Some(function) = &mut mutant.enclosing_function {
            function.span = (function.span.0 + start, function.span.1 + start);
        }
        output.mutants.push(mutant);
    }
    Ok(())
}

/// The spans that limit the sites of one block.
#[derive(Debug, Default)]
pub struct Protected {
    /// A site that overlaps one of these spans gives no mutant.
    overlap: Vec<(u32, u32)>,
    /// A site that contains one of these spans gives no mutant.
    contain: Vec<(u32, u32)>,
}

impl Protected {
    /// True when the site with `span` gives no mutant.
    pub fn excludes(&self, span: (u32, u32)) -> bool {
        self.overlap
            .iter()
            .any(|&(start, end)| start < span.1 && span.0 < end)
            || self
                .contain
                .iter()
                .any(|&(start, end)| span.0 <= start && end <= span.1)
    }
}

/// The Vue compiler macros. The compiler moves their arguments out of
/// `setup()`, or replaces the call. A switch around a macro call hides it
/// from the compiler, so the call and its arguments give no mutant.
const VUE_MACROS: [&str; 7] = [
    "defineProps",
    "defineEmits",
    "defineExpose",
    "defineModel",
    "defineOptions",
    "defineSlots",
    "withDefaults",
];

/// The Svelte runes. A rune call must stay in its own position.
const SVELTE_RUNES: [&str; 7] = [
    "$state",
    "$derived",
    "$effect",
    "$props",
    "$bindable",
    "$inspect",
    "$host",
];

/// The spans of `program` that limit the sites of a block with `role`.
pub fn protected_spans(program: &Program<'_>, role: ScriptRole) -> Protected {
    let mut protected = Protected::default();
    match role {
        ScriptRole::VueScript => {}
        ScriptRole::VueSetup => {
            let mut finder = CallFinder {
                names: &VUE_MACROS,
                declarators: true,
                spans: Vec::new(),
            };
            finder.visit_program(program);
            protected.overlap = finder.spans;
        }
        ScriptRole::SvelteModule | ScriptRole::SvelteInstance => {
            let mut finder = CallFinder {
                names: &SVELTE_RUNES,
                declarators: false,
                spans: Vec::new(),
            };
            finder.visit_program(program);
            protected.contain = finder.spans;
        }
        ScriptRole::AstroFrontmatter => {
            protected.overlap = program
                .body
                .iter()
                .filter(|statement| {
                    matches!(
                        statement,
                        Statement::ExportDeclaration(_) | Statement::ExportDefaultDeclaration(_)
                    )
                })
                .map(|statement| {
                    let span = statement.span();
                    (span.start, span.end)
                })
                .collect();
        }
    }
    protected
}

/// Finds the calls whose callee is one of `names`, or a member of one of
/// `names` (`$state.raw(...)`, `$effect.pre(...)`).
struct CallFinder {
    names: &'static [&'static str],
    /// Also gives the whole declarator when such a call is its initializer.
    /// The Vue compiler moves the destructure defaults of
    /// `const { size = 1 } = defineProps()` into the props options.
    declarators: bool,
    spans: Vec<(u32, u32)>,
}

impl CallFinder {
    fn matches(&self, callee: &Expression<'_>) -> bool {
        let mut current = callee;
        loop {
            match current {
                Expression::Identifier(identifier) => {
                    return self.names.contains(&identifier.name.as_str());
                }
                Expression::StaticMemberExpression(member) => current = &member.object,
                Expression::ParenthesizedExpression(inner) => current = &inner.expression,
                _ => return false,
            }
        }
    }
}

impl<'a> Visit<'a> for CallFinder {
    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        let call = it
            .init
            .as_ref()
            .and_then(|init| match init.without_parentheses() {
                Expression::CallExpression(call) => Some(call),
                _ => None,
            });
        if self.declarators && call.is_some_and(|call| self.matches(&call.callee)) {
            self.spans.push((it.span.start, it.span.end));
        }
        walk::walk_variable_declarator(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if self.matches(&it.callee) {
            self.spans.push((it.span.start, it.span.end));
        }
        walk::walk_call_expression(self, it);
    }
}
