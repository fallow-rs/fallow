//! The check that each source edit keeps the meaning of the code that it
//! keeps.
//!
//! `fallow fix`, the language server and the CI review comments remove code
//! that the analysis found unused. A guard before an edit protects one known
//! shape. This module compares the facts of the text before and after the
//! edit, so that an edit of a new shape gets the same check:
//!
//! - the edited text parses;
//! - each kept class element and enum member is still there, in the same
//!   class or enum and in the same order;
//! - each kept enum member keeps its constant value;
//! - no reference that resolved before the edit is unresolved after it;
//! - no import stays behind whose binding only a removed export list used.
//!
//! [`SourceFacts::exports`] gives the exported names of the text, so that a
//! caller can also check the exports that other files use.

use std::path::Path;

use oxc_ast::ast::{
    BinaryExpression, BinaryOperator, Class, ClassElement, ExportDefaultDeclarationKind,
    Expression, MethodDefinitionKind, Program, PropertyKey, Statement, TSEnumDeclaration,
    TSEnumMemberName, TSExternalModuleDeclaration, TSGlobalDeclaration, TSNamespaceDeclaration,
    UnaryOperator,
};
use oxc_ast_visit::Visit;
use oxc_span::GetSpan;
use oxc_syntax::symbol::SymbolFlags;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::source_edits::{MergeKind, SpanEdit, declaration_names, script_blocks};

/// Why the check refuses an edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostEditViolation {
    /// The edited text does not parse.
    DoesNotParse,
    /// A class or an enum outside the edited bytes is not in the edited text.
    DeclarationLost {
        /// The name of the class or the enum.
        name: String,
    },
    /// A class element or an enum member outside the edited bytes is not in
    /// the edited text, for example because the edit joined two members.
    MemberLost {
        /// The name of the class or the enum.
        container: String,
        /// The key of the lost member.
        member: String,
    },
    /// A kept enum member has a different constant value.
    ValueChanged {
        /// The name of the enum.
        enumeration: String,
        /// The name of the member.
        member: String,
    },
    /// A reference that resolved before the edit is unresolved after it.
    UnresolvedReference {
        /// The referenced name.
        name: String,
    },
    /// The edit removes an export that other code still uses. The caller
    /// reports this violation: the check of one file does not know the other
    /// files.
    ExportInUse {
        /// The exported name.
        name: String,
    },
    /// Declarations of one name that TypeScript merges are partly exported,
    /// as after the removal of `export` from one overload signature, or from
    /// a class but not from its interface. TypeScript requires all
    /// declarations of one merge to be exported, or none.
    MixedExport {
        /// The declared name.
        name: String,
    },
    /// An import stays, but the export list that used its binding is gone.
    ImportLeftBehind {
        /// The local name of the import binding.
        name: String,
    },
}

impl PostEditViolation {
    /// A short text for a person.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::DoesNotParse => "the edited file does not parse".to_owned(),
            Self::DeclarationLost { name } => format!("the edit also removes `{name}`"),
            Self::MemberLost { container, member } => {
                format!("the edit also removes `{member}` of `{container}`")
            }
            Self::ValueChanged {
                enumeration,
                member,
            } => format!("the edit changes the value of `{enumeration}.{member}`"),
            Self::UnresolvedReference { name } => {
                format!("the edit leaves a reference to `{name}` without a declaration")
            }
            Self::ExportInUse { name } => {
                format!("the edit removes the export `{name}`, and other code still uses it")
            }
            Self::MixedExport { name } => {
                format!("the edit leaves merged declarations of `{name}` partly exported")
            }
            Self::ImportLeftBehind { name } => {
                format!("the edit leaves the import of `{name}` with no use")
            }
        }
    }
}

/// The constant value of an enum member.
#[derive(Debug, Clone)]
enum ConstValue {
    Number(f64),
    Text(String),
}

impl PartialEq for ConstValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Number(left), Self::Number(right)) => left.to_bits() == right.to_bits(),
            (Self::Text(left), Self::Text(right)) => left == right,
            _ => false,
        }
    }
}

/// The kind (`true` for an enum) and the name of a class or an enum. An
/// anonymous class has the name `""`.
type ContainerKey = (bool, String);

/// One class or enum of a text.
#[derive(Debug)]
struct Container {
    /// The bytes of the whole declaration, in file offsets.
    span: (u32, u32),
    /// The key and the bytes of each member, in source order.
    members: Vec<(String, (u32, u32))>,
    /// The constant value of each member of an enum, by member key. `None`
    /// when the syntax does not give a constant value.
    values: FxHashMap<String, Option<ConstValue>>,
}

/// The facts of one file text that the check compares.
#[derive(Debug, Default)]
pub struct SourceFacts {
    /// The classes and enums of each kind and name, in source order.
    containers: FxHashMap<ContainerKey, Vec<Container>>,
    exports: FxHashSet<String>,
    unresolved: FxHashSet<String>,
    /// The number of references of each top-level import binding.
    import_uses: FxHashMap<String, usize>,
    /// The local names that an `export { ... }` list without `from` names.
    listed_locals: FxHashSet<String>,
    /// The merge kinds of the exported top-level declarations of each name,
    /// including a named `export default function` or `class`.
    exported_declarations: FxHashMap<String, FxHashSet<MergeKind>>,
    /// The merge kinds of the top-level declarations without `export`.
    local_declarations: FxHashMap<String, FxHashSet<MergeKind>>,
}

impl SourceFacts {
    /// The facts of `source`, the text of the file at `path`. `None` for a
    /// file type that the engine does not parse here, or for a text with a
    /// parse error.
    #[must_use]
    pub fn collect(path: &Path, source: &str) -> Option<Self> {
        let mut facts = Self::default();
        for block in script_blocks(path, source)? {
            let text = block.text.as_ref();
            let parsed = fallow_extract::parse_bounded(text, || {
                let allocator = oxc_allocator::Allocator::default();
                let parsed = oxc_parser::Parser::new(&allocator, text, block.source_type).parse();
                if parsed.fatal_error || parsed.diagnostics.has_errors() {
                    return false;
                }
                let mut collector = FactCollector {
                    text,
                    offset: block.offset,
                    ambient_depth: usize::from(block.source_type.is_typescript_definition()),
                    facts: &mut facts,
                };
                collector.visit_program(&parsed.program);
                collect_module_facts(&parsed.program, &mut facts);
                true
            });
            if parsed != Some(true) {
                return None;
            }
        }
        Some(facts)
    }

    /// Each name that the text exports. A default export has the name
    /// `default`.
    #[must_use]
    pub fn exports(&self) -> &FxHashSet<String> {
        &self.exports
    }

    /// Check `after`, the edited text of the file at `path`. `deleted` holds
    /// the byte ranges of the deletions in the text before the edit. A member
    /// or a declaration inside one of these ranges may go; each other member
    /// must stay.
    ///
    /// # Errors
    ///
    /// The first violation that the check finds.
    pub fn check(
        &self,
        path: &Path,
        after: &str,
        deleted: &[(u32, u32)],
    ) -> Result<Self, PostEditViolation> {
        let facts = Self::collect(path, after).ok_or(PostEditViolation::DoesNotParse)?;
        let inside = |span: (u32, u32)| {
            deleted
                .iter()
                .any(|&(start, end)| start <= span.0 && span.1 <= end)
        };
        let mut keys: Vec<&ContainerKey> = self.containers.keys().collect();
        keys.sort();
        for key in keys {
            let now = facts.containers.get(key).map_or(&[][..], Vec::as_slice);
            for (before, now) in pair_containers(&key.1, &self.containers[key], now, &inside)? {
                compare_members(&key.1, before, now, &inside)?;
                if key.0 {
                    compare_values(&key.1, before, now)?;
                }
            }
        }
        let mut unresolved: Vec<&String> = facts.unresolved.difference(&self.unresolved).collect();
        unresolved.sort();
        if let Some(name) = unresolved.first() {
            return Err(PostEditViolation::UnresolvedReference {
                name: (*name).clone(),
            });
        }
        let split_before = self.split_merges();
        let mut mixed: Vec<&String> = facts
            .split_merges()
            .into_iter()
            .filter(|name| !split_before.contains(name))
            .collect();
        mixed.sort();
        if let Some(name) = mixed.first() {
            return Err(PostEditViolation::MixedExport {
                name: (*name).clone(),
            });
        }
        let mut imports: Vec<(&String, &usize)> = facts.import_uses.iter().collect();
        imports.sort();
        for (name, uses) in imports {
            let used_before = self.import_uses.get(name).is_some_and(|before| *before > 0);
            if *uses == 0
                && used_before
                && self.listed_locals.contains(name)
                && !facts.listed_locals.contains(name)
            {
                return Err(PostEditViolation::ImportLeftBehind { name: name.clone() });
            }
        }
        Ok(facts)
    }

    /// The names with an exported and a local declaration that TypeScript
    /// merges, which it rejects (TS2383, TS2395).
    fn split_merges(&self) -> Vec<&String> {
        self.exported_declarations
            .iter()
            .filter(|(name, exported)| {
                self.local_declarations.get(*name).is_some_and(|local| {
                    exported
                        .iter()
                        .any(|kind| local.iter().any(|other| kind.merges_with(*other)))
                })
            })
            .map(|(name, _)| name)
            .collect()
    }

    /// Check `edits` of `source`, the text that these facts describe. See
    /// [`check_edits`].
    ///
    /// # Errors
    ///
    /// The first violation that the check finds.
    pub fn check_edits(
        &self,
        path: &Path,
        source: &str,
        edits: &[SpanEdit],
        removable: impl Fn(&str) -> bool,
    ) -> Result<(), PostEditViolation> {
        let after = apply(source, edits).ok_or(PostEditViolation::DoesNotParse)?;
        let deleted: Vec<(u32, u32)> = edits
            .iter()
            .filter(|edit| edit.replacement.is_empty())
            .map(|edit| (edit.start, edit.end))
            .collect();
        let facts = self.check(path, &after, &deleted)?;
        let mut removed: Vec<&String> = self.exports.difference(&facts.exports).collect();
        removed.sort();
        match removed.into_iter().find(|name| !removable(name)) {
            Some(name) => Err(PostEditViolation::ExportInUse { name: name.clone() }),
            None => Ok(()),
        }
    }
}

/// True for a file type that the check parses: JavaScript, TypeScript, and
/// the script blocks of Vue, Svelte and Astro files.
#[must_use]
pub fn checks_file(path: &Path) -> bool {
    script_blocks(path, "").is_some()
}

/// Check the edit of one file: `edits` applied to `source`, the text of the
/// file at `path`. `removable` tells which exported names the edit may
/// remove. The language server and the CI review comments call this check
/// before they offer an edit. `fallow fix` runs the same check over all the
/// files that it edits.
///
/// # Errors
///
/// The first violation that the check finds. An edit list with overlapping
/// or invalid ranges, or a `source` that does not parse, gives
/// [`PostEditViolation::DoesNotParse`].
pub fn check_edits(
    path: &Path,
    source: &str,
    edits: &[SpanEdit],
    removable: impl Fn(&str) -> bool,
) -> Result<(), PostEditViolation> {
    SourceFacts::collect(path, source)
        .ok_or(PostEditViolation::DoesNotParse)?
        .check_edits(path, source, edits, removable)
}

/// `source` with `edits` applied, or `None` when two edits overlap or an edit
/// is not a valid range of `source`.
fn apply(source: &str, edits: &[SpanEdit]) -> Option<String> {
    let mut sorted: Vec<&SpanEdit> = edits.iter().collect();
    sorted.sort_by_key(|edit| (edit.start, edit.end));
    let mut result = String::with_capacity(source.len());
    let mut position = 0;
    for edit in sorted {
        let (start, end) = (edit.start as usize, edit.end as usize);
        if start < position || end < start {
            return None;
        }
        result.push_str(source.get(position..start)?);
        result.push_str(&edit.replacement);
        source.get(start..end)?;
        position = end;
    }
    result.push_str(source.get(position..)?);
    Some(result)
}

/// Pair each class or enum of one kind and name after the edit with its
/// declaration before the edit. A declaration inside a deletion may go. When
/// fewer declarations stay, the first ones inside a deletion are the removed
/// ones.
fn pair_containers<'c>(
    name: &str,
    before: &'c [Container],
    now: &'c [Container],
    inside: &impl Fn((u32, u32)) -> bool,
) -> Result<Vec<(&'c Container, &'c Container)>, PostEditViolation> {
    let lost = || PostEditViolation::DeclarationLost {
        name: name.to_owned(),
    };
    let required = before
        .iter()
        .filter(|container| !inside(container.span))
        .count();
    if now.len() < required || now.len() > before.len() {
        return Err(lost());
    }
    let mut removed = before.len() - now.len();
    let kept = before.iter().filter(|container| {
        if removed > 0 && inside(container.span) {
            removed -= 1;
            return false;
        }
        true
    });
    Ok(kept.zip(now).collect())
}

/// Check that the member keys of `now` keep the order of `before`, and that
/// each member outside a deletion is still there. Two members can share a
/// key, such as an overload signature and its implementation, so the check
/// counts the members of each key.
fn compare_members(
    container: &str,
    before: &Container,
    now: &Container,
    inside: &impl Fn((u32, u32)) -> bool,
) -> Result<(), PostEditViolation> {
    let lost = |member: &str| PostEditViolation::MemberLost {
        container: container.to_owned(),
        member: member.to_owned(),
    };
    let mut remaining = before.members.iter();
    for (key, _) in &now.members {
        if !remaining.any(|(before_key, _)| before_key == key) {
            return Err(lost(key));
        }
    }
    let mut required: FxHashMap<&str, usize> = FxHashMap::default();
    for (key, span) in &before.members {
        if !inside(*span) {
            *required.entry(key.as_str()).or_default() += 1;
        }
    }
    let mut present: FxHashMap<&str, usize> = FxHashMap::default();
    for (key, _) in &now.members {
        *present.entry(key.as_str()).or_default() += 1;
    }
    let mut keys: Vec<(&str, usize)> = required.into_iter().collect();
    keys.sort_unstable();
    match keys
        .into_iter()
        .find(|(key, count)| present.get(key).copied().unwrap_or_default() < *count)
    {
        Some((key, _)) => Err(lost(key)),
        None => Ok(()),
    }
}

/// Check that each enum member with a known value in `before` has the same
/// value in `now`.
fn compare_values(
    enumeration: &str,
    before: &Container,
    now: &Container,
) -> Result<(), PostEditViolation> {
    for (key, _) in &now.members {
        let Some(Some(old)) = before.values.get(key) else {
            continue;
        };
        if now.values.get(key).and_then(Option::as_ref) != Some(old) {
            return Err(PostEditViolation::ValueChanged {
                enumeration: enumeration.to_owned(),
                member: key.clone(),
            });
        }
    }
    Ok(())
}

fn add_merge(map: &mut FxHashMap<String, FxHashSet<MergeKind>>, name: &str, kind: MergeKind) {
    map.entry(name.to_owned()).or_default().insert(kind);
}

/// Record the exports and the merge kinds of one top-level statement.
fn record_statement(statement: &Statement<'_>, facts: &mut SourceFacts) {
    match statement {
        Statement::ExportDeclaration(export) => {
            if let Some((_, names)) = declaration_names(&export.declaration) {
                if let Some(kind) = MergeKind::of(&export.declaration) {
                    for name in &names {
                        add_merge(&mut facts.exported_declarations, name, kind);
                    }
                }
                facts.exports.extend(names);
            }
        }
        Statement::ExportDefaultDeclaration(export) => {
            facts.exports.insert("default".to_owned());
            if let Some((name, kind)) = named_default(&export.declaration) {
                add_merge(&mut facts.exported_declarations, name, kind);
            }
        }
        Statement::ExportNamedDeclaration(export) => {
            for specifier in &export.specifiers {
                facts.exports.insert(specifier.exported.name().to_string());
                facts
                    .listed_locals
                    .insert(specifier.local.name().to_string());
            }
        }
        Statement::ExportFromDeclaration(export) => {
            for specifier in &export.specifiers {
                facts.exports.insert(specifier.exported.name().to_string());
            }
        }
        Statement::ExportAllDeclaration(export) => {
            if let Some(exported) = &export.exported {
                facts.exports.insert(exported.name().to_string());
            }
        }
        other => {
            if let Some(declaration) = other.as_declaration()
                && let Some(kind) = MergeKind::of(declaration)
                && let Some((_, names)) = declaration_names(declaration)
            {
                for name in &names {
                    add_merge(&mut facts.local_declarations, name, kind);
                }
            }
        }
    }
}

/// The name and the merge kind of a named `export default function` or
/// `export default class`.
fn named_default<'a>(
    declaration: &'a ExportDefaultDeclarationKind<'_>,
) -> Option<(&'a str, MergeKind)> {
    match declaration {
        ExportDefaultDeclarationKind::FunctionDeclaration(function) => function
            .id
            .as_ref()
            .map(|id| (id.name.as_str(), MergeKind::Function)),
        ExportDefaultDeclarationKind::ClassDeclaration(class) => class
            .id
            .as_ref()
            .map(|id| (id.name.as_str(), MergeKind::Class)),
        _ => None,
    }
}

/// Collect the exports, the unresolved references and the import uses of one
/// parsed block.
fn collect_module_facts(program: &Program<'_>, facts: &mut SourceFacts) {
    for statement in &program.body {
        record_statement(statement, facts);
    }
    let semantic = oxc_semantic::SemanticBuilder::new().build(program);
    let scoping = semantic.semantic.scoping();
    facts.unresolved.extend(
        scoping
            .root_unresolved_references()
            .keys()
            .map(ToString::to_string),
    );
    let root = scoping.root_scope_id();
    for symbol in scoping.symbol_ids() {
        if scoping.symbol_scope_id(symbol) != root
            || !scoping.symbol_flags(symbol).contains(SymbolFlags::Import)
        {
            continue;
        }
        *facts
            .import_uses
            .entry(scoping.symbol_name(symbol).to_string())
            .or_default() += scoping.get_resolved_reference_ids(symbol).len();
    }
}

/// Collects the classes and enums of one block.
struct FactCollector<'t, 'f> {
    text: &'t str,
    offset: u32,
    /// The number of enclosing ambient contexts: a declaration file, and each
    /// `declare` module, namespace or global block around the visit position.
    ambient_depth: usize,
    facts: &'f mut SourceFacts,
}

impl FactCollector<'_, '_> {
    fn span(&self, span: oxc_span::Span) -> (u32, u32) {
        (span.start + self.offset, span.end + self.offset)
    }

    fn slice(&self, span: oxc_span::Span) -> &str {
        self.text
            .get(span.start as usize..span.end as usize)
            .unwrap_or_default()
    }

    fn push(&mut self, is_enum: bool, name: String, container: Container) {
        self.facts
            .containers
            .entry((is_enum, name))
            .or_default()
            .push(container);
    }

    fn within(&mut self, declare: bool, walk: impl FnOnce(&mut Self)) {
        let depth = usize::from(declare);
        self.ambient_depth += depth;
        walk(self);
        self.ambient_depth -= depth;
    }

    fn element_key(&self, element: &ClassElement<'_>) -> String {
        let key = |kind: &str, is_static: bool, computed: bool, key: &PropertyKey<'_>| {
            let text = self.slice(key.span());
            format!(
                "{kind}{}{}{text}",
                if is_static { " static " } else { " " },
                if computed { "computed " } else { "" },
            )
        };
        match element {
            ClassElement::MethodDefinition(method) => {
                let kind = match method.kind {
                    MethodDefinitionKind::Constructor => "constructor",
                    MethodDefinitionKind::Method => "method",
                    MethodDefinitionKind::Get => "get",
                    MethodDefinitionKind::Set => "set",
                };
                key(kind, method.r#static, method.computed, &method.key)
            }
            ClassElement::PropertyDefinition(property) => key(
                "property",
                property.r#static,
                property.computed,
                &property.key,
            ),
            ClassElement::AccessorProperty(accessor) => key(
                "accessor",
                accessor.r#static,
                accessor.computed,
                &accessor.key,
            ),
            ClassElement::StaticBlock(_) => "static block".to_owned(),
            // The span of an index signature takes in the optional `;` or
            // `,` after it, also across a line break, so a separator that an
            // edit adds after the signature would change the key.
            ClassElement::TSIndexSignature(signature) => format!(
                "index {}",
                self.slice(signature.span)
                    .trim_end_matches(|c: char| c == ';' || c == ',' || c.is_whitespace())
            ),
        }
    }

    fn enum_member_key(&self, name: &TSEnumMemberName<'_>) -> String {
        match name {
            TSEnumMemberName::Identifier(id) => id.name.to_string(),
            TSEnumMemberName::String(name) | TSEnumMemberName::ComputedString(name) => name
                .value
                .as_str()
                .map_or_else(|| name.value.display().to_string(), str::to_owned),
            TSEnumMemberName::ComputedTemplateString(template) => {
                self.slice(template.span).to_owned()
            }
        }
    }
}

impl<'a> Visit<'a> for FactCollector<'_, '_> {
    fn visit_class(&mut self, it: &Class<'a>) {
        let members = it
            .body
            .body
            .iter()
            .map(|element| (self.element_key(element), self.span(element.span())))
            .collect();
        let name = it
            .id
            .as_ref()
            .map_or_else(String::new, |id| id.name.to_string());
        let container = Container {
            span: self.span(it.span),
            members,
            values: FxHashMap::default(),
        };
        self.push(false, name, container);
        oxc_ast_visit::walk::walk_class(self, it);
    }

    fn visit_ts_enum_declaration(&mut self, it: &TSEnumDeclaration<'a>) {
        let ambient = (it.declare || self.ambient_depth > 0) && !it.r#const;
        let name = it.id.name.to_string();
        let mut known: FxHashMap<String, ConstValue> = FxHashMap::default();
        let mut previous: Option<Option<ConstValue>> = None;
        let mut members = Vec::with_capacity(it.body.members.len());
        let mut values = FxHashMap::default();
        for member in &it.body.members {
            let key = self.enum_member_key(&member.id);
            let value = match &member.initializer {
                Some(initializer) => evaluate(initializer, &name, &known),
                None if ambient => None,
                None => match &previous {
                    None => Some(ConstValue::Number(0.0)),
                    Some(Some(ConstValue::Number(number))) => {
                        Some(ConstValue::Number(number + 1.0))
                    }
                    Some(_) => None,
                },
            };
            if let Some(value) = &value {
                known.insert(key.clone(), value.clone());
            }
            previous = Some(value.clone());
            values.insert(key.clone(), value);
            members.push((key, self.span(member.span)));
        }
        let container = Container {
            span: self.span(it.span),
            members,
            values,
        };
        self.push(true, name, container);
        oxc_ast_visit::walk::walk_ts_enum_declaration(self, it);
    }

    fn visit_ts_external_module_declaration(&mut self, it: &TSExternalModuleDeclaration<'a>) {
        self.within(it.declare, |collector| {
            oxc_ast_visit::walk::walk_ts_external_module_declaration(collector, it);
        });
    }

    fn visit_ts_namespace_declaration(&mut self, it: &TSNamespaceDeclaration<'a>) {
        self.within(it.declare, |collector| {
            oxc_ast_visit::walk::walk_ts_namespace_declaration(collector, it);
        });
    }

    fn visit_ts_global_declaration(&mut self, it: &TSGlobalDeclaration<'a>) {
        self.within(it.declare, |collector| {
            oxc_ast_visit::walk::walk_ts_global_declaration(collector, it);
        });
    }
}

/// The constant value of an enum initializer, as TypeScript computes it for
/// a constant enum expression. `known` holds the values of the members
/// before it. `None` for each other expression.
fn evaluate(
    expression: &Expression<'_>,
    enumeration: &str,
    known: &FxHashMap<String, ConstValue>,
) -> Option<ConstValue> {
    match expression {
        Expression::NumericLiteral(literal) => Some(ConstValue::Number(literal.value)),
        Expression::StringLiteral(literal) => {
            Some(ConstValue::Text(literal.value.as_str()?.to_owned()))
        }
        Expression::TemplateLiteral(template) if template.expressions.is_empty() => {
            let quasi = template.quasis.first()?;
            Some(ConstValue::Text(quasi.value.cooked?.as_str()?.to_owned()))
        }
        Expression::ParenthesizedExpression(inner) => {
            evaluate(&inner.expression, enumeration, known)
        }
        Expression::Identifier(identifier) => known.get(identifier.name.as_str()).cloned(),
        Expression::StaticMemberExpression(member) => match &member.object {
            Expression::Identifier(object) if object.name == enumeration => {
                known.get(member.property.name.as_str()).cloned()
            }
            _ => None,
        },
        Expression::ComputedMemberExpression(member) => {
            match (&member.object, &member.expression) {
                (Expression::Identifier(object), Expression::StringLiteral(property))
                    if object.name == enumeration =>
                {
                    known.get(property.value.as_str()?).cloned()
                }
                _ => None,
            }
        }
        Expression::UnaryExpression(unary) => {
            let ConstValue::Number(value) = evaluate(&unary.argument, enumeration, known)? else {
                return None;
            };
            let result = match unary.operator {
                UnaryOperator::UnaryPlus => value,
                UnaryOperator::UnaryNegation => -value,
                UnaryOperator::BitwiseNot => f64::from(!to_int32(value)),
                _ => return None,
            };
            Some(ConstValue::Number(result))
        }
        Expression::BinaryExpression(binary) => evaluate_binary(binary, enumeration, known),
        _ => None,
    }
}

fn evaluate_binary(
    binary: &BinaryExpression<'_>,
    enumeration: &str,
    known: &FxHashMap<String, ConstValue>,
) -> Option<ConstValue> {
    let left = evaluate(&binary.left, enumeration, known)?;
    let right = evaluate(&binary.right, enumeration, known)?;
    let (left, right) = match (left, right) {
        (ConstValue::Number(left), ConstValue::Number(right)) => (left, right),
        (ConstValue::Text(left), ConstValue::Text(right))
            if binary.operator == BinaryOperator::Addition =>
        {
            return Some(ConstValue::Text(format!("{left}{right}")));
        }
        _ => return None,
    };
    let shift = |value: f64| to_int32(value).cast_unsigned() & 31;
    let result = match binary.operator {
        BinaryOperator::Addition => left + right,
        BinaryOperator::Subtraction => left - right,
        BinaryOperator::Multiplication => left * right,
        BinaryOperator::Division => left / right,
        BinaryOperator::Remainder => left % right,
        BinaryOperator::Exponential => left.powf(right),
        BinaryOperator::ShiftLeft => f64::from(to_int32(left).wrapping_shl(shift(right))),
        BinaryOperator::ShiftRight => f64::from(to_int32(left).wrapping_shr(shift(right))),
        BinaryOperator::ShiftRightZeroFill => {
            f64::from(to_int32(left).cast_unsigned().wrapping_shr(shift(right)))
        }
        BinaryOperator::BitwiseAnd => f64::from(to_int32(left) & to_int32(right)),
        BinaryOperator::BitwiseOR => f64::from(to_int32(left) | to_int32(right)),
        BinaryOperator::BitwiseXOR => f64::from(to_int32(left) ^ to_int32(right)),
        _ => return None,
    };
    Some(ConstValue::Number(result))
}

/// The ECMAScript `ToInt32` conversion of a number.
fn to_int32(value: f64) -> i32 {
    const TWO_32: f64 = 4_294_967_296.0;
    if !value.is_finite() {
        return 0;
    }
    let wrapped = value.trunc().rem_euclid(TWO_32);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "rem_euclid gives an integer in 0..2^32"
    )]
    let unsigned = wrapped as u32;
    unsigned.cast_signed()
}

#[cfg(test)]
#[path = "post_edit_tests.rs"]
mod tests;
