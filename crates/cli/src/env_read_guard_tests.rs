//! The resolver reads each run variable of [`fallow_config::EnvVar`] from one
//! `EnvSnapshot` per run. A production file that names such a variable can
//! read it from the process environment by another rule. This test counts
//! each string literal whose value is the name of a run variable, in the
//! production code of every crate, outside `#[cfg(test)]` items and test
//! files. Literals in macro bodies and in attributes count too. Doc comments
//! do not count.
//!
//! [`NAME_OWNERS`] holds the files that may name a variable, each with the
//! exact count, so a new literal fails and a removed literal asks for a
//! smaller count. A message that names a variable inside a longer text is
//! not an exact match, so it does not count.

use fallow_config::EnvVar;
use proc_macro2::{TokenStream, TokenTree};
use rustc_hash::FxHashMap;
use syn::visit::{self, Visit};

use super::selection_key_guard_tests::{is_cfg_test, test_sources};
use crate::architecture_boundaries::rust_sources_under;

/// A production file that may name a run variable.
struct NameOwner {
    path: &'static str,
    literals: usize,
    why: &'static str,
}

const NAME_OWNERS: &[NameOwner] = &[
    NameOwner {
        path: "crates/config/src/run_inputs/env.rs",
        literals: 10,
        why: "the names of the variables that the snapshot reads",
    },
    NameOwner {
        path: "crates/config/src/production_env.rs",
        literals: 4,
        why: "the constants of the production variables",
    },
    NameOwner {
        path: "crates/config/src/config/resolution.rs",
        literals: 3,
        why: "the constants of the cache and package baseline variables",
    },
    NameOwner {
        path: "crates/cli/src/schema/environment.rs",
        literals: 15,
        why: "the schema of the environment variables names each one",
    },
    NameOwner {
        path: "crates/cli/src/report/ci/diff_filter.rs",
        literals: 2,
        why: "the output scope of the diff file, outside the run decisions",
    },
    NameOwner {
        path: "crates/cli/src/baselines.rs",
        literals: 2,
        why: "a diff file narrows the baseline prune run, and the error names the variable",
    },
    NameOwner {
        path: "crates/cli/src/report/suggestions.rs",
        literals: 1,
        why: "a next step is not offered when the diff file narrows it again",
    },
    NameOwner {
        path: "crates/cli/src/dispatch.rs",
        literals: 2,
        why: "the coverage variables are not run inputs yet",
    },
    NameOwner {
        path: "crates/mcp/src/tools/api_runtime.rs",
        literals: 2,
        why: "the coverage variables of the MCP tools are not run inputs yet",
    },
    NameOwner {
        path: "crates/mcp/src/tool_guides.rs",
        literals: 1,
        why: "the topic name of a tool guide",
    },
    NameOwner {
        path: "crates/api/src/audit_run/base_ref.rs",
        literals: 1,
        why: "the audit base of the callers without a resolved run",
    },
    NameOwner {
        path: "crates/api/src/runtime/audit_base_ref.rs",
        literals: 2,
        why: "the error context names the origin of the audit base",
    },
    NameOwner {
        path: "crates/api/src/coverage.rs",
        literals: 1,
        why: "the error context names the origin of the coverage root",
    },
];

/// The name of each run variable.
fn run_variable_names() -> Vec<&'static str> {
    EnvVar::ALL.into_iter().map(EnvVar::name).collect()
}

struct NameLiteralVisitor {
    names: Vec<&'static str>,
    literals: usize,
}

impl NameLiteralVisitor {
    fn count(&mut self, value: &str) {
        if self.names.contains(&value) {
            self.literals += 1;
        }
    }

    fn count_tokens(&mut self, tokens: TokenStream) {
        for token in tokens {
            match token {
                TokenTree::Group(group) => self.count_tokens(group.stream()),
                TokenTree::Literal(literal) => {
                    if let Ok(text) = syn::parse_str::<syn::LitStr>(&literal.to_string()) {
                        self.count(&text.value());
                    }
                }
                TokenTree::Ident(_) | TokenTree::Punct(_) => {}
            }
        }
    }
}

impl<'ast> Visit<'ast> for NameLiteralVisitor {
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
fn name_literals(source: &str) -> usize {
    let file = syn::parse_file(source).unwrap_or_else(|error| panic!("parse: {error}"));
    let mut visitor = NameLiteralVisitor {
        names: run_variable_names(),
        literals: 0,
    };
    visitor.visit_file(&file);
    visitor.literals
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
        let literals = name_literals(&read_source(&source_path));
        if literals > 0 {
            found.insert(source_path, literals);
        }
    }
    let mut mismatches = Vec::new();
    for (path, literals) in &found {
        match NAME_OWNERS.iter().find(|owner| owner.path == path) {
            Some(owner) if owner.literals == *literals => {}
            Some(owner) => mismatches.push(format!(
                "{path}: {literals} literals, NAME_OWNERS allows {} ({})",
                owner.literals, owner.why
            )),
            None => mismatches.push(format!(
                "{path}: {literals} literals; read the variable from the EnvSnapshot of the run"
            )),
        }
    }
    for owner in NAME_OWNERS {
        if !found.contains_key(owner.path) {
            mismatches.push(format!(
                "{}: no literal is left; remove the file from NAME_OWNERS",
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
    assert_eq!(name_literals(source), 4);
}
