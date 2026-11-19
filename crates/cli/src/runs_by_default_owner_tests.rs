//! `fallow_config::plan_run` decides whether a run executes duplication.
//! `DuplicatesConfig::runs_by_default` reads the config only, so a caller
//! outside `fallow-config` can decide the duplication analysis by another
//! rule. This test counts each call of it in the production code of every
//! crate outside `fallow-config`, outside `#[cfg(test)]` items and test
//! files.
//!
//! The owner is `crates/engine/src/health/derived_sections.rs`: the health
//! score measures duplication when the config enables it, whatever analyses
//! one run selects. [`CALLERS`] also lists the CLI sites that still read the
//! config in place of the plan of the run, each with the exact count.

use rustc_hash::FxHashMap;
use syn::visit::{self, Visit};

use super::selection_key_guard_tests::{is_cfg_test, test_sources};
use crate::architecture_boundaries::rust_sources_under;

struct Caller {
    path: &'static str,
    calls: usize,
    why: &'static str,
}

const CALLERS: &[Caller] = &[
    Caller {
        path: "crates/engine/src/health/derived_sections.rs",
        calls: 1,
        why: "the health score does not depend on the analyses that one run selects",
    },
    Caller {
        path: "crates/cli/src/dispatch_audit_inputs.rs",
        calls: 1,
        why: "an audit whose config turns duplication off counts as a full audit",
    },
    Caller {
        path: "crates/cli/src/combined/impact.rs",
        calls: 1,
        why: "a bare run whose config turns duplication off counts as a whole-project run",
    },
    Caller {
        path: "crates/api/src/editor.rs",
        calls: 1,
        why: "an editor session that is not opened from a resolved run",
    },
];

const METHOD: &str = "runs_by_default";

#[derive(Default)]
struct CallVisitor {
    calls: usize,
}

impl<'ast> Visit<'ast> for CallVisitor {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        let attrs: &[syn::Attribute] = match item {
            syn::Item::Fn(item) => &item.attrs,
            syn::Item::Impl(item) => &item.attrs,
            syn::Item::Mod(item) => &item.attrs,
            syn::Item::Const(item) => &item.attrs,
            syn::Item::Static(item) => &item.attrs,
            _ => &[],
        };
        if !is_cfg_test(attrs) {
            visit::visit_item(self, item);
        }
    }

    fn visit_impl_item(&mut self, item: &'ast syn::ImplItem) {
        let skip = matches!(item, syn::ImplItem::Fn(function) if is_cfg_test(&function.attrs));
        if !skip {
            visit::visit_impl_item(self, item);
        }
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if call.method == METHOD {
            self.calls += 1;
        }
        visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
        if path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == METHOD)
        {
            self.calls += 1;
        }
        visit::visit_expr_path(self, path);
    }
}

fn calls_in(source_path: &str) -> usize {
    let root = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
    )
    .join("../..");
    let source = std::fs::read_to_string(root.join(source_path))
        .unwrap_or_else(|error| panic!("read {source_path}: {error}"));
    let file =
        syn::parse_file(&source).unwrap_or_else(|error| panic!("parse {source_path}: {error}"));
    let mut visitor = CallVisitor::default();
    visitor.visit_file(&file);
    visitor.calls
}

#[test]
fn runs_by_default_has_one_owner_outside_config() {
    let test_sources = test_sources();
    let mut found = FxHashMap::default();
    for source_path in rust_sources_under(["crates"]) {
        if source_path.starts_with("crates/config/")
            || source_path.split('/').nth(2) != Some("src")
            || test_sources.contains(&source_path)
        {
            continue;
        }
        let calls = calls_in(&source_path);
        if calls > 0 {
            found.insert(source_path, calls);
        }
    }
    let mut mismatches = Vec::new();
    for (path, calls) in &found {
        match CALLERS.iter().find(|caller| caller.path == path) {
            Some(caller) if caller.calls == *calls => {}
            Some(caller) => mismatches.push(format!(
                "{path}: {calls} calls, CALLERS allows {} ({})",
                caller.calls, caller.why
            )),
            None => mismatches.push(format!(
                "{path}: {calls} calls; read the duplication slot of the resolved run"
            )),
        }
    }
    for caller in CALLERS {
        if !found.contains_key(caller.path) {
            mismatches.push(format!(
                "{}: no call is left; remove the site from CALLERS",
                caller.path
            ));
        }
    }
    mismatches.sort();
    assert!(mismatches.is_empty(), "{mismatches:#?}");
}
