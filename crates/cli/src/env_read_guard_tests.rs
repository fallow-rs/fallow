//! The resolver reads each run variable of [`fallow_config::EnvVar`] from one
//! `EnvSnapshot` per run. A production file that names such a variable can
//! read it from the process environment by another rule. This test counts
//! each name of a run variable in the production code of every crate,
//! outside `#[cfg(test)]` items and test files. These forms name a variable:
//! - a string literal whose value is the name,
//! - a use of a name constant of [`NAME_CONSTANTS`], such as
//!   `fallow_config::PRODUCTION_ENV`,
//! - an `EnvVar::<variant>.name()` call,
//! - a path of [`NAME_PATHS`], such as `ProductionEnv::from_lookup`.
//!
//! The forms in macro bodies and in attributes count too. Doc comments do not
//! count.
//!
//! [`NAME_OWNERS`] holds the files that may name a variable, each with the
//! exact count, so a new name fails and a removed name asks for a smaller
//! count. A message that names a variable inside a longer text is not an
//! exact match, so it does not count.

use fallow_config::EnvVar;
use proc_macro2::{TokenStream, TokenTree};
use rustc_hash::FxHashMap;
use syn::visit::{self, Visit};

use super::selection_key_guard_tests::{is_cfg_test, test_sources};
use crate::architecture_boundaries::rust_sources_under;

/// A production file that may name a run variable.
struct NameOwner {
    path: &'static str,
    names: usize,
    why: &'static str,
}

const NAME_OWNERS: &[NameOwner] = &[
    NameOwner {
        path: "crates/config/src/run_inputs/env.rs",
        names: 17,
        why: "the names of the variables that the snapshot reads",
    },
    NameOwner {
        path: "crates/config/src/production_env.rs",
        names: 8,
        why: "the constants of the production variables and the lookup that parses them",
    },
    NameOwner {
        path: "crates/config/src/config/resolution.rs",
        names: 4,
        why: "the constants of the cache and package baseline variables, and \
              cache_max_size_env_override, which reads the process environment",
    },
    NameOwner {
        path: "crates/engine/src/resolved_run/pick.rs",
        names: 4,
        why: "the source of a production mode names the variable of the snapshot value",
    },
    NameOwner {
        path: "crates/engine/src/resolved_run/resolve.rs",
        names: 1,
        why: "the source of the cache directory names the variable of the snapshot value",
    },
    NameOwner {
        path: "crates/api/src/run.rs",
        names: 1,
        why: "the error of an invalid type-aware variable names it",
    },
    NameOwner {
        path: "crates/api/src/runtime/type_aware_config.rs",
        names: 1,
        why: "the error of a type-aware layer that the caller did not request names the variable",
    },
    NameOwner {
        path: "crates/cli/src/schema/environment.rs",
        names: 17,
        why: "the schema of the environment variables names each one",
    },
    NameOwner {
        path: "crates/cli/src/report/ci/diff_filter.rs",
        names: 2,
        why: "the output scope of the diff file, outside the run decisions",
    },
    NameOwner {
        path: "crates/cli/src/baselines.rs",
        names: 2,
        why: "a diff file narrows the baseline prune run, and the error names the variable",
    },
    NameOwner {
        path: "crates/cli/src/report/suggestions.rs",
        names: 1,
        why: "a next step is not offered when the diff file narrows it again",
    },
    NameOwner {
        path: "crates/cli/src/dispatch.rs",
        names: 2,
        why: "the coverage variables are not run inputs yet",
    },
    NameOwner {
        path: "crates/mcp/src/tools/api_runtime.rs",
        names: 2,
        why: "the coverage variables of the MCP tools are not run inputs yet",
    },
    NameOwner {
        path: "crates/mcp/src/tool_guides.rs",
        names: 1,
        why: "the topic name of a tool guide",
    },
    NameOwner {
        path: "crates/api/src/audit_run/base_ref.rs",
        names: 1,
        why: "the audit base of the callers without a resolved run",
    },
    NameOwner {
        path: "crates/api/src/runtime/audit_base_ref.rs",
        names: 2,
        why: "the error context names the origin of the audit base",
    },
    NameOwner {
        path: "crates/api/src/coverage.rs",
        names: 1,
        why: "the error context names the origin of the coverage root",
    },
];

/// The public constants of `fallow-config` whose value is the name of a run
/// variable.
const NAME_CONSTANTS: &[&str] = &[
    "PRODUCTION_ENV",
    "PRODUCTION_DEAD_CODE_ENV",
    "PRODUCTION_HEALTH_ENV",
    "PRODUCTION_DUPES_ENV",
    "CACHE_DIR_ENV",
    "CACHE_MAX_SIZE_ENV",
    "PACKAGE_BASELINES_ENV",
];

/// Paths that give the name of a run variable: `EnvVar::name` as a function
/// value, and `ProductionEnv::from_lookup`, which asks its lookup for the
/// production variables.
const NAME_PATHS: &[[&str; 2]] = &[["EnvVar", "name"], ["ProductionEnv", "from_lookup"]];

/// The name of each run variable.
fn run_variable_names() -> Vec<&'static str> {
    EnvVar::ALL.into_iter().map(EnvVar::name).collect()
}

struct NameVisitor {
    variables: Vec<&'static str>,
    names: usize,
}

/// One token of a macro body, without its span.
enum Token {
    Ident(String),
    Punct(char),
    Other,
}

impl NameVisitor {
    fn count(&mut self, value: &str) {
        if self.variables.contains(&value) {
            self.names += 1;
        }
    }

    fn count_tokens(&mut self, tokens: TokenStream) {
        let mut flat = Vec::new();
        for token in tokens {
            match token {
                TokenTree::Group(group) => {
                    self.count_tokens(group.stream());
                    flat.push(Token::Other);
                }
                TokenTree::Literal(literal) => {
                    if let Ok(text) = syn::parse_str::<syn::LitStr>(&literal.to_string()) {
                        self.count(&text.value());
                    }
                    flat.push(Token::Other);
                }
                TokenTree::Ident(ident) => flat.push(Token::Ident(ident.to_string())),
                TokenTree::Punct(punct) => flat.push(Token::Punct(punct.as_char())),
            }
        }
        for index in 0..flat.len() {
            if names_a_variable(&flat[..=index]) {
                self.names += 1;
            }
        }
    }
}

/// Whether the last token of `tokens` ends a name constant, an
/// `EnvVar::<variant>.name` call or a path of [`NAME_PATHS`].
fn names_a_variable(tokens: &[Token]) -> bool {
    let Some((Token::Ident(last), before)) = tokens.split_last() else {
        return false;
    };
    if NAME_CONSTANTS.contains(&last.as_str()) {
        return true;
    }
    let after_path = |owner: &str, rest: &[Token]| {
        matches!(
            rest,
            [.., Token::Ident(ident), Token::Punct(':'), Token::Punct(':')] if ident == owner
        )
    };
    let name_call = last == "name"
        && matches!(before, [rest @ .., Token::Ident(_), Token::Punct('.')] if after_path("EnvVar", rest));
    name_call
        || NAME_PATHS
            .iter()
            .any(|[owner, item]| last == item && after_path(owner, before))
}

/// Whether `path` ends with `owner::item`.
fn path_ends_with(path: &syn::Path, owner: &str, item: &str) -> bool {
    let mut segments = path.segments.iter().rev();
    segments.next().is_some_and(|segment| segment.ident == item)
        && segments
            .next()
            .is_some_and(|segment| segment.ident == owner)
}

impl<'ast> Visit<'ast> for NameVisitor {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        if !is_cfg_test(item_attrs(item)) {
            visit::visit_item(self, item);
        }
    }

    fn visit_impl_item(&mut self, item: &'ast syn::ImplItem) {
        let skip = matches!(item, syn::ImplItem::Fn(function) if is_cfg_test(&function.attrs));
        if !skip {
            visit::visit_impl_item(self, item);
        }
    }

    fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
        if attribute.path().is_ident("doc") {
            return;
        }
        if let syn::Meta::List(list) = &attribute.meta {
            self.count_tokens(list.tokens.clone());
        }
        visit::visit_attribute(self, attribute);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        self.count_tokens(mac.tokens.clone());
    }

    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        self.count(&literal.value());
    }

    fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
        let constant = path
            .path
            .segments
            .last()
            .is_some_and(|segment| NAME_CONSTANTS.iter().any(|name| segment.ident == name));
        let name_path = NAME_PATHS
            .iter()
            .any(|[owner, item]| path_ends_with(&path.path, owner, item));
        if constant || name_path {
            self.names += 1;
        }
        visit::visit_expr_path(self, path);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let variant_name = call.method == "name"
            && call.args.is_empty()
            && matches!(
                &*call.receiver,
                syn::Expr::Path(receiver)
                    if receiver.path.segments.iter().rev().nth(1).is_some_and(|owner| owner.ident == "EnvVar")
            );
        if variant_name {
            self.names += 1;
        }
        visit::visit_expr_method_call(self, call);
    }
}

fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Const(item) => &item.attrs,
        syn::Item::Enum(item) => &item.attrs,
        syn::Item::Fn(item) => &item.attrs,
        syn::Item::Impl(item) => &item.attrs,
        syn::Item::Macro(item) => &item.attrs,
        syn::Item::Mod(item) => &item.attrs,
        syn::Item::Static(item) => &item.attrs,
        syn::Item::Struct(item) => &item.attrs,
        syn::Item::Trait(item) => &item.attrs,
        syn::Item::Type(item) => &item.attrs,
        syn::Item::Use(item) => &item.attrs,
        _ => &[],
    }
}

/// The number of run variable names in the production code of `source`.
fn variable_names(source: &str) -> usize {
    let file = syn::parse_file(source).unwrap_or_else(|error| panic!("parse: {error}"));
    let mut visitor = NameVisitor {
        variables: run_variable_names(),
        names: 0,
    };
    visitor.visit_file(&file);
    visitor.names
}

fn production_sources() -> Vec<String> {
    let test_sources = test_sources();
    rust_sources_under(["crates"])
        .into_iter()
        .filter(|path| path.split('/').nth(2) == Some("src"))
        .filter(|path| !test_sources.contains(path))
        .collect()
}

fn read_source(source_path: &str) -> String {
    let root = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
    )
    .join("../..");
    std::fs::read_to_string(root.join(source_path))
        .unwrap_or_else(|error| panic!("read {source_path}: {error}"))
}

#[test]
fn only_the_owners_name_a_run_variable() {
    let mut found = FxHashMap::default();
    for source_path in production_sources() {
        let names = variable_names(&read_source(&source_path));
        if names > 0 {
            found.insert(source_path, names);
        }
    }
    let mut mismatches = Vec::new();
    for (path, names) in &found {
        match NAME_OWNERS.iter().find(|owner| owner.path == path) {
            Some(owner) if owner.names == *names => {}
            Some(owner) => mismatches.push(format!(
                "{path}: {names} names, NAME_OWNERS allows {} ({})",
                owner.names, owner.why
            )),
            None => mismatches.push(format!(
                "{path}: {names} names; read the variable from the EnvSnapshot of the run"
            )),
        }
    }
    for owner in NAME_OWNERS {
        if !found.contains_key(owner.path) {
            mismatches.push(format!(
                "{}: no name is left; remove the file from NAME_OWNERS",
                owner.path
            ));
        }
    }
    mismatches.sort();
    assert!(mismatches.is_empty(), "{mismatches:#?}");
}

#[test]
fn the_visitor_counts_code_macros_and_attributes_but_not_docs_or_tests() {
    let source = r#"
        /// FALLOW_PRODUCTION
        const NAME: &str = "FALLOW_PRODUCTION";
        fn read() -> Option<String> {
            let _ = format!("{}", "FALLOW_DIFF_FILE");
            std::env::var("FALLOW_AUDIT_BASE").ok()
        }
        #[arg(env = "FALLOW_COVERAGE")]
        struct Args;
        fn message() -> &'static str {
            "set FALLOW_PRODUCTION to true"
        }
        fn other() -> &'static str {
            "FALLOW_TYPE_AWARE_BACKEND"
        }
        #[cfg(test)]
        fn in_test() -> &'static str {
            "FALLOW_PRODUCTION"
        }
    "#;
    assert_eq!(variable_names(source), 4);
}

#[test]
fn the_visitor_counts_the_name_constants_and_the_name_calls() {
    let source = r#"
        use fallow_config::{CACHE_DIR_ENV, PRODUCTION_ENV};
        fn read(other: Other) {
            let _ = std::env::var(fallow_config::PRODUCTION_ENV);
            let _ = std::env::var_os(CACHE_DIR_ENV);
            let _ = std::env::var_os(fallow_config::EnvVar::CacheDir.name());
            let _ = ProductionEnv::from_lookup(|name| std::env::var(name).ok());
            let _ = EnvVar::ALL.map(EnvVar::name);
            assert!(std::env::var(PRODUCTION_HEALTH_ENV).is_ok(), "set");
            let _ = vec![EnvVar::CacheMaxSize.name(); 2];
            let _ = format!("set {PRODUCTION_ENV}");
            let _ = other.name();
            let _ = std::env::var_os(other.label());
        }
        #[cfg(test)]
        fn in_test() -> Option<String> {
            std::env::var(PRODUCTION_ENV).ok()
        }
    "#;
    assert_eq!(variable_names(source), 7);
}
