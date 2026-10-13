//! Instrumented text emission.
//!
//! The emitter copies the original text and replaces each site with a
//! mutation switch:
//!
//! ```text
//! (__fallow_m_a("ID1") ? (MUT1) : __fallow_m_a("ID2") ? (MUT2)
//!   : (__fallow_m_c("ID1"), __fallow_m_c("ID2"), (ORIGINAL)))
//! ```
//!
//! A mutated branch copies the raw original text of the site with one region
//! replaced. Only the original branch holds the instrumented inner sites, so
//! inner mutants live only inside the original branch of an outer mutant.
//! A conditional expression introduces no function boundary, so `this`,
//! `arguments`, `super`, `await` and `yield` keep their meaning.

use std::collections::BTreeSet;

use crate::{ACTIVE_FUNCTION, HIT_FUNCTION, SpliceMap, SpliceSegment, is_identifier_byte};

/// One mutant of a site, ready to emit.
pub struct EmitMutant {
    pub id: String,
    pub region: (u32, u32),
    pub replacement: String,
}

/// One site, ready to emit.
pub struct EmitSite {
    pub span: (u32, u32),
    pub mutants: Vec<EmitMutant>,
}

/// Text that the emitter inserts at an original offset.
pub struct Insertion {
    pub offset: u32,
    pub text: String,
}

struct Writer<'s> {
    source: &'s str,
    out: String,
    segments: Vec<SpliceSegment>,
}

impl Writer<'_> {
    fn copy(&mut self, from: u32, to: u32) {
        if to <= from {
            return;
        }
        let Some(text) = self.source.get(from as usize..to as usize) else {
            return;
        };
        let generated_start = self.out.len() as u32;
        let len = to - from;
        match self.segments.last_mut() {
            Some(last)
                if last.generated_start + last.len == generated_start
                    && last.original_start + last.len == from =>
            {
                last.len += len;
            }
            _ => self.segments.push(SpliceSegment {
                generated_start,
                original_start: from,
                len,
            }),
        }
        self.out.push_str(text);
    }

    fn push(&mut self, text: &str) {
        self.out.push_str(text);
    }
}

struct Tree {
    children: Vec<Vec<usize>>,
    roots: Vec<usize>,
}

/// Builds the containment tree. `sites` is ordered by start, then by end
/// descending, and site spans nest or are disjoint.
fn build_tree(sites: &[EmitSite]) -> Tree {
    let mut children = vec![Vec::new(); sites.len()];
    let mut roots = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    for (index, site) in sites.iter().enumerate() {
        while let Some(&top) = stack.last() {
            let parent = sites[top].span;
            if parent.0 <= site.span.0 && site.span.1 <= parent.1 {
                break;
            }
            stack.pop();
        }
        match stack.last() {
            Some(&parent) => children[parent].push(index),
            None => roots.push(index),
        }
        stack.push(index);
    }
    Tree { children, roots }
}

struct Emitter<'s, 't> {
    writer: Writer<'s>,
    sites: &'t [EmitSite],
    tree: Tree,
    statement_starts: &'t BTreeSet<u32>,
}

impl Emitter<'_, '_> {
    fn range(&mut self, from: u32, to: u32, nodes: &[usize], parent_start: Option<u32>) {
        let mut cursor = from;
        for &node in nodes {
            let span = self.sites[node].span;
            self.writer.copy(cursor, span.0);
            self.site(node, parent_start);
            cursor = span.1;
        }
        self.writer.copy(cursor, to);
    }

    fn site(&mut self, node: usize, parent_start: Option<u32>) {
        let sites = self.sites;
        let site = &sites[node];
        let (start, end) = site.span;
        // A statement that starts with `(` can join the line before it.
        if self.statement_starts.contains(&start) && parent_start != Some(start) {
            // `do++i` has no space after the keyword; `dovoid` would be a name.
            let joins = self
                .writer
                .out
                .bytes()
                .last()
                .is_some_and(is_identifier_byte);
            self.writer.push(if joins { " void " } else { "void " });
        }
        self.writer.push("(");
        for mutant in &site.mutants {
            self.writer
                .push(&format!("{ACTIVE_FUNCTION}(\"{}\") ? (", mutant.id));
            self.writer.copy(start, mutant.region.0);
            self.writer.push(&mutant.replacement);
            self.writer.copy(mutant.region.1, end);
            self.writer.push(") : ");
        }
        self.writer.push("(");
        for mutant in &site.mutants {
            self.writer
                .push(&format!("{HIT_FUNCTION}(\"{}\"), ", mutant.id));
        }
        self.writer.push("(");
        let children = std::mem::take(&mut self.tree.children[node]);
        self.range(start, end, &children, Some(start));
        self.writer.push(")))");
    }
}

/// Emits the instrumented text and its splice map.
pub fn emit(
    source: &str,
    sites: &[EmitSite],
    statement_starts: &BTreeSet<u32>,
    insertions: &[Insertion],
) -> (String, SpliceMap) {
    let tree = build_tree(sites);
    let mut emitter = Emitter {
        writer: Writer {
            source,
            out: String::with_capacity(source.len() * 2),
            segments: Vec::new(),
        },
        sites,
        tree,
        statement_starts,
    };
    let mut cursor = 0;
    for insertion in insertions {
        emitter.writer.copy(cursor, insertion.offset);
        emitter.writer.push(&insertion.text);
        cursor = insertion.offset;
    }
    let roots = std::mem::take(&mut emitter.tree.roots);
    emitter.range(cursor, source.len() as u32, &roots, None);
    let Writer { out, segments, .. } = emitter.writer;
    (out, SpliceMap { segments })
}
