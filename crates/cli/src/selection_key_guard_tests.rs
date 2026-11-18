//! `fallow_config::plan_run` is the one rule that decides which of the dead
//! code, duplication and health analyses a run executes. A surface that reads
//! a selection key itself can apply another precedence, so these tests reject
//! each such read outside the `fallow-config` crate.
//!
//! The selection keys are `duplicates.enabled` and the `audit.deadCode`,
//! `audit.health` and `audit.dupes` keys.
//!
//! Two checks cover the keys:
//! - A text needle finds `.duplicates.enabled` in every Rust file, test files
//!   and macro bodies included.
//! - A syntax visitor finds each read of a selection key in production code.
//!
//! The visitor knows that a binding holds a config section from one of these:
//! - the declared type of the binding,
//! - an untyped `let` whose value is a section, such as `config.audit`,
//! - a destructure of a `duplicates` or `audit` field, such as
//!   `let ResolvedConfig { duplicates, .. } = config`,
//! - an untyped closure parameter of a method on a value that wraps a
//!   section, such as `config.map(|c| &c.audit).and_then(|audit| audit.dupes)`.
//!
//! Known limit: the visitor cannot see the return type of a method, so
//! `let audit = config.audit_section(); audit.dupes` passes the guard.

use rustc_hash::{FxHashMap, FxHashSet};
use syn::visit::{self, Visit};

use crate::architecture_boundaries::{read_source_without_line_comments, rust_sources_under};

/// A production file outside `fallow-config` that may read a selection key.
struct SelectionKeyOwner {
    path: &'static str,
    /// Each read that the file owns, as `<base>.<field>`, once for each read.
    reads: &'static [&'static str],
    fact: &'static str,
}

const SELECTION_KEY_OWNERS: &[SelectionKeyOwner] = &[SelectionKeyOwner {
    path: "crates/lsp/src/initialization.rs",
    reads: &["config.enabled"],
    fact: "the copy of duplicates.enabled into the duplication config of the editor",
}];

#[test]
fn only_the_config_crate_reads_the_duplicates_enabled_field() {
    let needle = concat!(".duplicates", ".enabled");
    let offenders: Vec<String> = rust_sources_under(["crates"])
        .into_iter()
        .filter(|source_path| !source_path.starts_with("crates/config/"))
        .filter(|source_path| {
            read_source_without_line_comments(source_path)
                .unwrap_or_else(|error| panic!("read {source_path}: {error}"))
                .split_whitespace()
                .collect::<String>()
                .contains(needle)
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "decide the duplication analysis with fallow_config::plan_run, not with the enabled field: {offenders:?}"
    );
}

#[test]
fn only_the_config_crate_reads_a_selection_key() {
    let test_sources = test_sources();
    let mut offenders = Vec::new();
    for source_path in rust_sources_under(["crates"]) {
        if source_path.starts_with("crates/config/") || test_sources.contains(&source_path) {
            continue;
        }
        let reads = selection_key_reads_in_file(&source_path);
        offenders.extend(unowned_reads(&source_path, reads, SELECTION_KEY_OWNERS));
    }
    assert!(
        offenders.is_empty(),
        "decide the analyses of a run with fallow_config::plan_run, not with a selection key, or name the read and the fact the file owns in SELECTION_KEY_OWNERS: {offenders:#?}"
    );

    for owner in SELECTION_KEY_OWNERS {
        assert!(
            !owner.fact.is_empty(),
            "{} must state the fact it owns",
            owner.path
        );
        let missing = missing_owned_reads(owner, &selection_key_reads_in_file(owner.path));
        assert!(
            missing.is_empty(),
            "{} no longer reads {missing:?}; remove the read from its SELECTION_KEY_OWNERS entry",
            owner.path
        );
    }
}

/// Each read in `reads` of the file `source_path` that no owner entry names,
/// as `<path>:<line>: <base>.<field>`. An entry names one read only, so a
/// second read with the same text is not owned.
fn unowned_reads(
    source_path: &str,
    reads: Vec<String>,
    owners: &[SelectionKeyOwner],
) -> Vec<String> {
    let mut owned: Vec<&str> = owners
        .iter()
        .filter(|owner| owner.path == source_path)
        .flat_map(|owner| owner.reads.iter().copied())
        .collect();
    reads
        .into_iter()
        .filter(|read| {
            let Some(index) = owned.iter().position(|key| *key == read_key(read)) else {
                return true;
            };
            owned.swap_remove(index);
            false
        })
        .map(|read| format!("{source_path}:{read}"))
        .collect()
}

/// Each read that `owner` names and that `reads` does not hold.
fn missing_owned_reads(owner: &SelectionKeyOwner, reads: &[String]) -> Vec<&'static str> {
    let mut found: Vec<&str> = reads.iter().map(|read| read_key(read)).collect();
    owner
        .reads
        .iter()
        .copied()
        .filter(|key| {
            let Some(index) = found.iter().position(|read| read == key) else {
                return true;
            };
            found.swap_remove(index);
            false
        })
        .collect()
}

/// The `<base>.<field>` part of a `<line>: <base>.<field>` read.
fn read_key(read: &str) -> &str {
    read.split_once(": ").map_or(read, |(_, key)| key)
}

/// Each Rust file under `crates` that holds tests only: a file in a `tests`
/// folder, a file with `#![cfg(test)]`, and each file that a `#[cfg(test)]`
/// module declaration pulls in, with the files that it declares in turn.
fn test_sources() -> &'static FxHashSet<String> {
    static TEST_SOURCES: std::sync::OnceLock<FxHashSet<String>> = std::sync::OnceLock::new();
    TEST_SOURCES.get_or_init(collect_test_sources)
}

fn collect_test_sources() -> FxHashSet<String> {
    let mut test_sources = FxHashSet::default();
    let mut pending = Vec::new();
    for source_path in rust_sources_under(["crates"]) {
        let Some(file) = parse_source_file(&source_path) else {
            continue;
        };
        if is_test_source(&source_path) || is_cfg_test(&file.attrs) {
            pending.push(source_path);
            continue;
        }
        pending.extend(declared_module_files(&source_path, &file.items, false));
    }
    while let Some(source_path) = pending.pop() {
        if !test_sources.insert(source_path.clone()) {
            continue;
        }
        if let Some(file) = parse_source_file(&source_path) {
            pending.extend(declared_module_files(&source_path, &file.items, true));
        }
    }
    test_sources
}

/// The parsed file, or `None` when the file does not parse. The production
/// scan reports a production file that does not parse.
fn parse_source_file(source_path: &str) -> Option<syn::File> {
    let source = std::fs::read_to_string(workspace_root().join(source_path)).ok()?;
    syn::parse_file(&source).ok()
}

/// The files of the module declarations in `items` of the file `source_path`.
/// With `all`, each declaration counts. Otherwise only a declaration under
/// `#[cfg(test)]` counts.
fn declared_module_files(source_path: &str, items: &[syn::Item], all: bool) -> Vec<String> {
    let (dir, file_name) = source_path.rsplit_once('/').unwrap_or(("", source_path));
    let own_dir = if matches!(file_name, "mod.rs" | "lib.rs" | "main.rs") {
        dir.to_owned()
    } else {
        format!("{dir}/{}", file_name.trim_end_matches(".rs"))
    };
    let mut files = Vec::new();
    collect_declared_module_files(dir, &own_dir, items, all, &mut files);
    files
}

/// `dir` is the folder of the declaring file, for a `#[path]` attribute.
/// `module_dir` is the folder of the child modules of `items`.
fn collect_declared_module_files(
    dir: &str,
    module_dir: &str,
    items: &[syn::Item],
    all: bool,
    out: &mut Vec<String>,
) {
    for item in items {
        let syn::Item::Mod(module) = item else {
            continue;
        };
        let counts = all || is_cfg_test(&module.attrs);
        let name = module.ident.to_string();
        if let Some((_, inner)) = &module.content {
            let inner_dir = format!("{module_dir}/{name}");
            collect_declared_module_files(&inner_dir, &inner_dir, inner, counts, out);
            continue;
        }
        if !counts {
            continue;
        }
        let candidates = match path_attribute(&module.attrs) {
            Some(path) => vec![format!("{dir}/{path}")],
            None => vec![
                format!("{module_dir}/{name}.rs"),
                format!("{module_dir}/{name}/mod.rs"),
                format!("{dir}/{name}.rs"),
                format!("{dir}/{name}/mod.rs"),
            ],
        };
        if let Some(file) = candidates
            .into_iter()
            .map(|candidate| normalize_path(&candidate))
            .find(|candidate| workspace_root().join(candidate).is_file())
        {
            out.push(file);
        }
    }
}

/// The value of a `#[path = "..."]` attribute.
fn path_attribute(attrs: &[syn::Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| {
        let syn::Meta::NameValue(name_value) = &attr.meta else {
            return None;
        };
        if !name_value.path.is_ident("path") {
            return None;
        }
        let syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(value),
            ..
        }) = &name_value.value
        else {
            return None;
        };
        Some(value.value())
    })
}

/// `path` without `.` parts, and with each `..` part applied.
fn normalize_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    parts.join("/")
}

fn selection_key_reads_in_file(source_path: &str) -> Vec<String> {
    let source = std::fs::read_to_string(workspace_root().join(source_path))
        .unwrap_or_else(|error| panic!("read {source_path}: {error}"));
    selection_key_reads(&source).unwrap_or_else(|error| panic!("parse {source_path}: {error}"))
}

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
    )
    .join("../..")
}

/// Whether a file is in a `tests` folder. Cargo builds such a file as an
/// integration test, without a module declaration in production code.
fn is_test_source(source_path: &str) -> bool {
    source_path.split('/').any(|part| part == "tests")
}

/// Each read of a selection key in `source`, as `<line>: <base>.<field>`.
fn selection_key_reads(source: &str) -> syn::Result<Vec<String>> {
    let file = syn::parse_file(source)?;
    let mut visitor = SelectionKeyVisitor::default();
    visitor.visit_file(&file);
    Ok(visitor.reads)
}

/// A config section that holds selection keys.
#[derive(Clone, Copy)]
enum Section {
    Audit,
    Duplicates,
}

impl Section {
    /// The section that a field access with this name gives, as in
    /// `config.audit`.
    fn from_field(name: &str) -> Option<Self> {
        match name {
            "audit" => Some(Self::Audit),
            "duplicates" => Some(Self::Duplicates),
            _ => None,
        }
    }

    /// The section of a declared type, as in `cfg: &fallow_config::AuditConfig`.
    fn from_type(ty: &syn::Type) -> Option<Self> {
        match ty {
            syn::Type::Reference(reference) => Self::from_type(&reference.elem),
            syn::Type::Paren(paren) => Self::from_type(&paren.elem),
            syn::Type::Path(type_path) => Self::from_path(&type_path.path),
            _ => None,
        }
    }

    fn from_path(path: &syn::Path) -> Option<Self> {
        let name = path.segments.last()?.ident.to_string();
        if name.ends_with("AuditConfig") {
            Some(Self::Audit)
        } else if name.ends_with("DuplicatesConfig") {
            Some(Self::Duplicates)
        } else {
            None
        }
    }

    fn covers(self, field: &str) -> bool {
        match self {
            Self::Audit => matches!(field, "dead_code" | "health" | "dupes"),
            Self::Duplicates => field == "enabled",
        }
    }
}

/// The methods that give the section of their receiver.
const KEEP_SECTION_METHODS: &[&str] = &[
    "as_deref",
    "as_mut",
    "as_ref",
    "clone",
    "cloned",
    "copied",
    "expect",
    "filter",
    "inspect",
    "into_iter",
    "iter",
    "ok",
    "unwrap",
    "unwrap_or_default",
];

/// The methods that give the section of their closure value.
const MAP_SECTION_METHODS: &[&str] = &["and_then", "filter_map", "flat_map", "map"];

#[derive(Default)]
struct SelectionKeyVisitor {
    /// The bindings of a config section in each open function, innermost last.
    scopes: Vec<FxHashMap<String, Section>>,
    reads: Vec<String>,
}

impl SelectionKeyVisitor {
    fn binding(&self, name: &str) -> Option<Section> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    fn bind(&mut self, name: String, section: Section) {
        if self.scopes.is_empty() {
            self.scopes.push(FxHashMap::default());
        }
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, section);
        }
    }

    /// The section that `expr` gives, when `expr` is a section field access or
    /// a section binding.
    fn section_of(&self, expr: &syn::Expr) -> Option<Section> {
        match expr {
            syn::Expr::Field(field) => match &field.member {
                syn::Member::Named(name) => Section::from_field(&name.to_string()),
                syn::Member::Unnamed(_) => None,
            },
            syn::Expr::Path(path) if path.qself.is_none() => {
                let ident = path.path.get_ident()?;
                self.binding(&ident.to_string())
            }
            syn::Expr::Reference(reference) => self.section_of(&reference.expr),
            syn::Expr::Paren(paren) => self.section_of(&paren.expr),
            syn::Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
                self.section_of(&unary.expr)
            }
            syn::Expr::MethodCall(call) => self.section_of_method_call(call),
            _ => None,
        }
    }

    /// The section that a value which wraps a section gives, as in
    /// `config.map(|c| &c.audit)` or `audit.as_ref()`.
    fn section_of_method_call(&self, call: &syn::ExprMethodCall) -> Option<Section> {
        let method = call.method.to_string();
        if KEEP_SECTION_METHODS.contains(&method.as_str()) {
            return self.section_of(&call.receiver);
        }
        if !MAP_SECTION_METHODS.contains(&method.as_str()) {
            return None;
        }
        call.args.iter().find_map(|arg| match arg {
            syn::Expr::Closure(closure) => self.section_of(&closure.body),
            _ => None,
        })
    }

    fn record(&mut self, line: usize, base: &str, field: &str) {
        self.reads.push(format!("{line}: {base}.{field}"));
    }

    fn in_scope(&mut self, walk: impl FnOnce(&mut Self)) {
        self.scopes.push(FxHashMap::default());
        walk(self);
        self.scopes.pop();
    }

    /// Scan the tokens of a macro body for `<base> . <field>`. A base after a
    /// `.` is a field access, and other bases are bindings.
    fn scan_tokens(&mut self, cursor: syn::buffer::Cursor<'_>) {
        let mut tokens: Vec<Token> = Vec::new();
        let mut cursor = cursor;
        while !cursor.eof() {
            if let Some((ident, next)) = cursor.ident() {
                tokens.push(Token::Ident(ident.to_string(), ident.span().start().line));
                cursor = next;
            } else if let Some((punct, next)) = cursor.punct() {
                tokens.push(if punct.as_char() == '.' {
                    Token::Dot
                } else {
                    Token::Other
                });
                cursor = next;
            } else if let Some((inside, _, _, next)) = cursor.any_group() {
                self.scan_tokens(inside);
                tokens.push(Token::Other);
                cursor = next;
            } else if let Some((_, next)) = cursor.token_tree() {
                tokens.push(Token::Other);
                cursor = next;
            } else {
                break;
            }
        }
        for (index, window) in tokens.windows(3).enumerate() {
            let [Token::Ident(base, line), Token::Dot, Token::Ident(field, _)] = window else {
                continue;
            };
            let after_dot = index > 0 && matches!(tokens[index - 1], Token::Dot);
            let section = if after_dot {
                Section::from_field(base)
            } else {
                self.binding(base)
            };
            if section.is_some_and(|section| section.covers(field)) {
                self.record(*line, base, field);
            }
        }
    }
}

/// One token of a macro body. A group counts as one other token.
enum Token {
    Ident(String, usize),
    Dot,
    Other,
}

fn is_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("cfg") {
            return false;
        }
        let syn::Meta::List(list) = &attr.meta else {
            return false;
        };
        let text = list.tokens.to_string();
        let words: Vec<&str> = text
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .collect();
        words.contains(&"test") && !words.contains(&"not")
    })
}

const fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Const(item) => item.attrs.as_slice(),
        syn::Item::Enum(item) => item.attrs.as_slice(),
        syn::Item::ExternCrate(item) => item.attrs.as_slice(),
        syn::Item::Fn(item) => item.attrs.as_slice(),
        syn::Item::ForeignMod(item) => item.attrs.as_slice(),
        syn::Item::Impl(item) => item.attrs.as_slice(),
        syn::Item::Macro(item) => item.attrs.as_slice(),
        syn::Item::Mod(item) => item.attrs.as_slice(),
        syn::Item::Static(item) => item.attrs.as_slice(),
        syn::Item::Struct(item) => item.attrs.as_slice(),
        syn::Item::Trait(item) => item.attrs.as_slice(),
        syn::Item::TraitAlias(item) => item.attrs.as_slice(),
        syn::Item::Type(item) => item.attrs.as_slice(),
        syn::Item::Union(item) => item.attrs.as_slice(),
        syn::Item::Use(item) => item.attrs.as_slice(),
        _ => &[],
    }
}

const fn impl_item_attrs(item: &syn::ImplItem) -> &[syn::Attribute] {
    match item {
        syn::ImplItem::Const(item) => item.attrs.as_slice(),
        syn::ImplItem::Fn(item) => item.attrs.as_slice(),
        syn::ImplItem::Type(item) => item.attrs.as_slice(),
        syn::ImplItem::Macro(item) => item.attrs.as_slice(),
        _ => &[],
    }
}

impl<'ast> Visit<'ast> for SelectionKeyVisitor {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        if !is_cfg_test(item_attrs(item)) {
            visit::visit_item(self, item);
        }
    }

    fn visit_impl_item(&mut self, item: &'ast syn::ImplItem) {
        if !is_cfg_test(impl_item_attrs(item)) {
            visit::visit_impl_item(self, item);
        }
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.in_scope(|visitor| visit::visit_item_fn(visitor, item));
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.in_scope(|visitor| visit::visit_impl_item_fn(visitor, item));
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        self.in_scope(|visitor| visit::visit_trait_item_fn(visitor, item));
    }

    fn visit_pat_type(&mut self, pat_type: &'ast syn::PatType) {
        if let syn::Pat::Ident(pat) = pat_type.pat.as_ref()
            && let Some(section) = Section::from_type(&pat_type.ty)
        {
            self.bind(pat.ident.to_string(), section);
        }
        visit::visit_pat_type(self, pat_type);
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        visit::visit_local(self, local);
        if let syn::Pat::Ident(pat) = &local.pat
            && let Some(init) = &local.init
            && let Some(section) = self.section_of(&init.expr)
        {
            self.bind(pat.ident.to_string(), section);
        }
    }

    fn visit_expr_field(&mut self, expr: &'ast syn::ExprField) {
        if let syn::Member::Named(field) = &expr.member {
            let field_name = field.to_string();
            if self
                .section_of(&expr.base)
                .is_some_and(|section| section.covers(&field_name))
            {
                let base = match expr.base.as_ref() {
                    syn::Expr::Field(base) => match &base.member {
                        syn::Member::Named(name) => name.to_string(),
                        syn::Member::Unnamed(_) => String::new(),
                    },
                    syn::Expr::Path(path) => path
                        .path
                        .get_ident()
                        .map(ToString::to_string)
                        .unwrap_or_default(),
                    _ => String::new(),
                };
                self.record(field.span().start().line, &base, &field_name);
            }
        }
        visit::visit_expr_field(self, expr);
    }

    fn visit_pat_struct(&mut self, pat: &'ast syn::PatStruct) {
        for field in &pat.fields {
            if let syn::Member::Named(name) = &field.member
                && let Some(section) = Section::from_field(&name.to_string())
                && let syn::Pat::Ident(binding) = field.pat.as_ref()
            {
                self.bind(binding.ident.to_string(), section);
            }
        }
        if let Some(section) = Section::from_path(&pat.path) {
            for field in &pat.fields {
                let syn::Member::Named(name) = &field.member else {
                    continue;
                };
                let field_name = name.to_string();
                if section.covers(&field_name) && !matches!(field.pat.as_ref(), syn::Pat::Wild(_)) {
                    let type_name = pat
                        .path
                        .segments
                        .last()
                        .map(|segment| segment.ident.to_string())
                        .unwrap_or_default();
                    self.record(name.span().start().line, &type_name, &field_name);
                }
            }
        }
        visit::visit_pat_struct(self, pat);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let receiver_section = self.section_of(&call.receiver);
        for attr in &call.attrs {
            self.visit_attribute(attr);
        }
        self.visit_expr(&call.receiver);
        if let Some(turbofish) = &call.turbofish {
            self.visit_angle_bracketed_generic_arguments(turbofish);
        }
        for arg in &call.args {
            match (arg, receiver_section) {
                (syn::Expr::Closure(closure), Some(section)) => self.in_scope(|visitor| {
                    for input in &closure.inputs {
                        if let syn::Pat::Ident(param) = input {
                            visitor.bind(param.ident.to_string(), section);
                        }
                    }
                    visitor.visit_expr_closure(closure);
                }),
                _ => self.visit_expr(arg),
            }
        }
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        let buffer = syn::buffer::TokenBuffer::new2(mac.tokens.clone());
        self.scan_tokens(buffer.begin());
        visit::visit_macro(self, mac);
    }
}

fn reads(source: &str) -> Vec<String> {
    selection_key_reads(source).expect("inline source parses")
}

#[test]
fn guard_flags_a_read_through_an_untyped_section_binding() {
    let source = "fn f(config: FallowConfig) -> bool { let audit_cfg = config.audit; audit_cfg.dupes.is_some() }";
    assert_eq!(reads(source), ["1: audit_cfg.dupes"]);
}

#[test]
fn guard_flags_a_read_through_a_typed_binding() {
    let source = "fn f(cfg: &fallow_config::AuditConfig) -> bool {\n    cfg.health.is_some()\n}";
    assert_eq!(reads(source), ["2: cfg.health"]);
}

#[test]
fn guard_flags_a_read_through_a_section_field() {
    // The split literal keeps the text needle quiet on this file.
    let source = concat!(
        "fn f(config: &ResolvedConfig) -> bool { config.duplicates",
        ".enabled && config.audit.dead_code.is_none() }"
    );
    assert_eq!(
        reads(source),
        ["1: duplicates.enabled", "1: audit.dead_code"]
    );
}

#[test]
fn guard_flags_a_read_inside_a_macro() {
    let source = "fn f(config: &FallowConfig) -> bool { let audit_cfg = &config.audit; matches!(audit_cfg.dupes, Some(false)) }";
    assert_eq!(reads(source), ["1: audit_cfg.dupes"]);
    let source = "fn f(config: &FallowConfig) { assert!(config.audit.health.is_none()); }";
    assert_eq!(reads(source), ["1: audit.health"]);
}

#[test]
fn guard_flags_a_destructure_that_binds_a_key() {
    let source = "fn f(cfg: &AuditConfig) { let AuditConfig { dupes, health: h, .. } = cfg; }";
    assert_eq!(
        reads(source),
        ["1: AuditConfig.dupes", "1: AuditConfig.health"]
    );
    let source = "fn f(cfg: &AuditConfig) { let AuditConfig { dupes: _, health: _, .. } = cfg; }";
    assert!(reads(source).is_empty());
}

#[test]
fn guard_does_not_flag_a_struct_literal_write() {
    let source = "fn f() -> DuplicatesConfig { DuplicatesConfig { enabled: true, ..DuplicatesConfig::default() } }";
    assert!(reads(source).is_empty());
}

#[test]
fn guard_skips_test_code() {
    let source = concat!(
        "#[cfg(test)]\nmod tests {\n    fn f(cfg: &AuditConfig) -> bool { cfg.dupes.is_some() }\n}\n",
        "impl X {\n    #[cfg(test)]\n    fn g(config: &FallowConfig) -> bool { config.duplicates",
        ".enabled }\n}"
    );
    assert!(reads(source).is_empty());
    assert!(is_test_source("crates/cli/tests/integration/main.rs"));
    assert!(!is_test_source("crates/cli/src/dispatch_audit_inputs.rs"));
}

#[test]
fn guard_resolves_the_file_of_a_module_declaration() {
    let file = syn::parse_file(concat!(
        "mod production;\n",
        "#[cfg(test)]\n#[path = \"run_plan_tests.rs\"]\nmod tests;\n",
        "#[cfg(test)]\nmod inline { mod nested; }\n",
    ))
    .expect("inline source parses");
    let source_path = "crates/config/src/config/run_plan.rs";
    assert_eq!(
        declared_module_files(source_path, &file.items, false),
        ["crates/config/src/config/run_plan_tests.rs"]
    );
    assert_eq!(normalize_path("crates/a/./b/../c.rs"), "crates/a/c.rs");
}

#[test]
fn guard_does_not_flag_fields_of_other_types() {
    let source = "fn f(analyses: AuditAnalysisSelection, args: &AuditDispatchArgs) -> bool { analyses.dupes || args.dupes || args.health.is_some() || format!(\"{}\", analyses.dead_code).is_empty() }";
    assert!(reads(source).is_empty());
}

#[test]
fn guard_flags_a_read_through_a_destructured_parent_config() {
    let source = "fn f(config: &ResolvedConfig) -> bool { let ResolvedConfig { duplicates, .. } = config; duplicates.enabled }";
    assert_eq!(reads(source), ["1: duplicates.enabled"]);
    let source = "fn f(config: &FallowConfig) -> bool { let FallowConfig { audit: a, .. } = config; a.dupes.is_some() }";
    assert_eq!(reads(source), ["1: a.dupes"]);
}

#[test]
fn guard_flags_a_read_through_an_untyped_closure_parameter() {
    let source = "fn f(config: Option<&ResolvedConfig>) -> bool { config.map(|c| &c.audit).and_then(|audit| audit.dupes).unwrap_or(true) }";
    assert_eq!(reads(source), ["1: audit.dupes"]);
    let source = "fn f(load: Option<FallowConfig>) -> Option<bool> { let audit = load.map(|config| config.audit); audit.as_ref().and_then(|a| a.health) }";
    assert_eq!(reads(source), ["1: a.health"]);
}

#[test]
fn guard_does_not_bind_a_closure_parameter_of_another_value() {
    let source =
        "fn f(args: Option<&AuditDispatchArgs>) -> bool { args.map(|a| a.dupes).unwrap_or(false) }";
    assert!(reads(source).is_empty());
}

#[test]
fn guard_flags_a_read_in_an_owner_file_that_the_owner_does_not_name() {
    let owner = &SELECTION_KEY_OWNERS[0];
    let found = vec![
        "148: config.enabled".to_owned(),
        "200: cfg.dupes".to_owned(),
    ];
    assert_eq!(
        unowned_reads(owner.path, found, SELECTION_KEY_OWNERS),
        [format!("{}:200: cfg.dupes", owner.path)]
    );
    let twice = vec![
        "148: config.enabled".to_owned(),
        "300: config.enabled".to_owned(),
    ];
    assert_eq!(
        unowned_reads(owner.path, twice, SELECTION_KEY_OWNERS),
        [format!("{}:300: config.enabled", owner.path)]
    );
    assert!(
        unowned_reads(
            owner.path,
            vec!["9: config.enabled".to_owned()],
            SELECTION_KEY_OWNERS
        )
        .is_empty()
    );
}

#[test]
fn guard_reports_an_owned_read_that_the_file_no_longer_has() {
    let owner = &SELECTION_KEY_OWNERS[0];
    assert_eq!(
        missing_owned_reads(owner, &["148: other.enabled".to_owned()]),
        ["config.enabled"]
    );
    assert!(missing_owned_reads(owner, &["148: config.enabled".to_owned()]).is_empty());
}

#[test]
fn guard_skips_each_file_that_a_cfg_test_module_declares() {
    let test_sources = test_sources();
    for path in [
        "crates/lsp/src/tests.rs",
        "crates/lsp/src/tests_clone_columns.rs",
        "crates/graph/src/graph/re_exports/tests_name_search.rs",
        "crates/core/src/analyze/unused_deps_tests/mod.rs",
        "crates/core/src/analyze/unused_deps_tests/helpers.rs",
        "crates/cli/src/selection_key_guard_tests.rs",
        "crates/cli/tests/integration/main.rs",
    ] {
        assert!(test_sources.contains(path), "{path} is a test source");
    }
    for path in [
        "crates/cli/src/dispatch_audit_inputs.rs",
        "crates/lsp/src/initialization.rs",
        "crates/graph/src/graph/re_exports/propagate.rs",
    ] {
        assert!(!test_sources.contains(path), "{path} is production code");
    }
}
