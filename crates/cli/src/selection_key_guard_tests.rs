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
//! Known limit: the visitor knows the type of a binding only from its declared
//! type, or from an untyped `let` whose value is a `duplicates` or `audit`
//! field access. It cannot see the return type of a method, so
//! `let audit = config.audit_section(); audit.dupes` passes the guard.

use rustc_hash::FxHashMap;
use syn::visit::{self, Visit};

use crate::architecture_boundaries::{read_source_without_line_comments, rust_sources_under};

/// Each production file outside `fallow-config` that may read a selection
/// key, and the fact it owns.
const SELECTION_KEY_OWNERS: &[(&str, &str)] = &[(
    "crates/lsp/src/initialization.rs",
    "the copy of duplicates.enabled into the duplication config of the editor",
)];

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
    let mut offenders = Vec::new();
    for source_path in rust_sources_under(["crates"]) {
        if source_path.starts_with("crates/config/") || is_test_source(&source_path) {
            continue;
        }
        if SELECTION_KEY_OWNERS
            .iter()
            .any(|(owner, _)| *owner == source_path)
        {
            continue;
        }
        for read in selection_key_reads_in_file(&source_path) {
            offenders.push(format!("{source_path}:{read}"));
        }
    }
    assert!(
        offenders.is_empty(),
        "decide the analyses of a run with fallow_config::plan_run, not with a selection key, or name the fact the file owns in SELECTION_KEY_OWNERS: {offenders:#?}"
    );

    for (owner, fact) in SELECTION_KEY_OWNERS {
        assert!(!fact.is_empty(), "{owner} must state the fact it owns");
        assert!(
            !selection_key_reads_in_file(owner).is_empty(),
            "{owner} no longer reads a selection key; remove its SELECTION_KEY_OWNERS entry"
        );
    }
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

/// Whether a file holds tests only: a file in a `tests` folder, or a file
/// named `tests.rs` or `*_tests.rs`.
fn is_test_source(source_path: &str) -> bool {
    let file_name = source_path.rsplit('/').next().unwrap_or(source_path);
    source_path.split('/').any(|part| part == "tests")
        || file_name == "tests.rs"
        || file_name.ends_with("_tests.rs")
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
            _ => None,
        }
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
    assert!(is_test_source("crates/lsp/src/tests.rs"));
    assert!(is_test_source(
        "crates/cli/src/cli_production/selection_key_guard_tests.rs"
    ));
    assert!(!is_test_source("crates/cli/src/dispatch_audit_inputs.rs"));
}

#[test]
fn guard_does_not_flag_fields_of_other_types() {
    let source = "fn f(analyses: AuditAnalysisSelection, args: &AuditDispatchArgs) -> bool { analyses.dupes || args.dupes || args.health.is_some() || format!(\"{}\", analyses.dead_code).is_empty() }";
    assert!(reads(source).is_empty());
}
