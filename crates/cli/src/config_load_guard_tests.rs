//! The CLI resolves each command through `fallow_engine::resolved_run`. A
//! production file that loads the config itself, or that opens a session
//! from a config without a resolved run, can decide the production mode or a
//! setting by another rule. This test counts each such call in the
//! production code of `crates/cli/src`, outside `#[cfg(test)]` items and test
//! files.
//!
//! The calls are `load_config`, `load_config_for_analysis`,
//! `load_config_file`, `AnalysisSession::from_resolved_config` and the
//! `config_for_project*` functions. [`OPEN_SITES`] holds the sites that still call them, each with
//! the exact count, so a new call fails and a removed call asks for a smaller
//! count.

use rustc_hash::FxHashMap;
use syn::visit::{self, Visit};

use super::selection_key_guard_tests::{is_cfg_test, test_sources};
use crate::architecture_boundaries::rust_sources_under;

/// A production file that still loads the config outside the resolver.
struct OpenSite {
    path: &'static str,
    calls: usize,
    why: &'static str,
}

const OPEN_SITES: &[OpenSite] = &[
    OpenSite {
        path: "crates/cli/src/runtime_support.rs",
        calls: 4,
        why: "the fallback load of an option struct without a run, the shared file load \
              of the sites below, and `loaded_config` for one optional setting",
    },
    OpenSite {
        path: "crates/cli/src/audit_cache.rs",
        calls: 1,
        why: "the fingerprint of the config file in the audit cache key",
    },
    OpenSite {
        path: "crates/cli/src/cli_report.rs",
        calls: 1,
        why: "the load check of the `--config` file of a saved report",
    },
    OpenSite {
        path: "crates/cli/src/config.rs",
        calls: 1,
        why: "`fallow config` prints the config file as the loader gives it",
    },
    OpenSite {
        path: "crates/cli/src/check/configuration.rs",
        calls: 1,
        why: "a dead-code run without a run: benchmark hooks",
    },
    OpenSite {
        path: "crates/cli/src/check/mod.rs",
        calls: 1,
        why: "a dead-code session without a run",
    },
    OpenSite {
        path: "crates/cli/src/combined/mod.rs",
        calls: 1,
        why: "a bare run without a duplication run",
    },
    OpenSite {
        path: "crates/cli/src/dupes.rs",
        calls: 1,
        why: "a duplication run without a run: benchmark hooks",
    },
    OpenSite {
        path: "crates/cli/src/health/mod.rs",
        calls: 2,
        why: "a health run without a run, and its session: benchmark hooks",
    },
    OpenSite {
        path: "crates/cli/src/cli_production.rs",
        calls: 1,
        why: "a dead-code command without a run: benchmark hooks",
    },
    OpenSite {
        path: "crates/cli/src/fix/mod.rs",
        calls: 1,
        why: "a fix without a run: benchmark hooks",
    },
    OpenSite {
        path: "crates/cli/src/flags.rs",
        calls: 1,
        why: "a flags run without a run: benchmark hooks",
    },
    OpenSite {
        path: "crates/cli/src/list.rs",
        calls: 1,
        why: "a list without a run: benchmark hooks",
    },
    OpenSite {
        path: "crates/cli/src/suppressions.rs",
        calls: 1,
        why: "a suppressions run without a run: benchmark hooks",
    },
    OpenSite {
        path: "crates/cli/src/viz/mod.rs",
        calls: 1,
        why: "a viz run without a run: benchmark hooks",
    },
];

/// The functions that load a config or open a session without a run.
fn is_load_call(name: &str) -> bool {
    matches!(
        name,
        "load_config" | "load_config_for_analysis" | "load_config_file" | "from_resolved_config"
    ) || name.starts_with("config_for_project")
}

#[derive(Default)]
struct LoadCallVisitor {
    calls: usize,
}

impl<'ast> Visit<'ast> for LoadCallVisitor {
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

    fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
        if path
            .path
            .segments
            .last()
            .is_some_and(|segment| is_load_call(&segment.ident.to_string()))
        {
            self.calls += 1;
        }
        visit::visit_expr_path(self, path);
    }
}

fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Fn(item) => &item.attrs,
        syn::Item::Mod(item) => &item.attrs,
        syn::Item::Impl(item) => &item.attrs,
        syn::Item::Const(item) => &item.attrs,
        syn::Item::Static(item) => &item.attrs,
        syn::Item::Struct(item) => &item.attrs,
        syn::Item::Enum(item) => &item.attrs,
        syn::Item::Use(item) => &item.attrs,
        _ => &[],
    }
}

fn load_calls(source_path: &str) -> usize {
    let root = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
    )
    .join("../..");
    let source = std::fs::read_to_string(root.join(source_path))
        .unwrap_or_else(|error| panic!("read {source_path}: {error}"));
    let file =
        syn::parse_file(&source).unwrap_or_else(|error| panic!("parse {source_path}: {error}"));
    let mut visitor = LoadCallVisitor::default();
    visitor.visit_file(&file);
    visitor.calls
}

#[test]
fn no_cli_production_file_loads_config_outside_the_resolver() {
    let test_sources = test_sources();
    let mut found = FxHashMap::default();
    for source_path in rust_sources_under(["crates/cli/src"]) {
        if test_sources.contains(&source_path) {
            continue;
        }
        let calls = load_calls(&source_path);
        if calls > 0 {
            found.insert(source_path, calls);
        }
    }
    let mut mismatches = Vec::new();
    for (path, calls) in &found {
        match OPEN_SITES.iter().find(|site| site.path == path) {
            Some(site) if site.calls == *calls => {}
            Some(site) => mismatches.push(format!(
                "{path}: {calls} calls, OPEN_SITES allows {} ({})",
                site.calls, site.why
            )),
            None => mismatches.push(format!(
                "{path}: {calls} calls; resolve the run with fallow_engine::resolved_run"
            )),
        }
    }
    for site in OPEN_SITES {
        if !found.contains_key(site.path) {
            mismatches.push(format!(
                "{}: no call is left; remove the site from OPEN_SITES",
                site.path
            ));
        }
    }
    mismatches.sort();
    assert!(mismatches.is_empty(), "{mismatches:#?}");
}
