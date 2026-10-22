//! Dependency trace tests.

use std::path::{Path, PathBuf};

use fallow_graph::resolve::{ResolveResult, ResolvedImport, ResolvedModule};
use fallow_types::extract::{ImportInfo, ImportedName};
use fallow_types::trace::PeerClosureSeeds;
use rustc_hash::FxHashSet;

use super::{DependencyCredit, trace_dependency};
use crate::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
use crate::graph::ModuleGraph;

#[test]
fn trace_dependency_used() {
    let files = vec![DiscoveredFile {
        id: FileId(0),
        path: PathBuf::from("/project/src/app.ts"),
        size_bytes: 100,
    }];
    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/src/app.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];
    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/src/app.ts"),
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "lodash".to_string(),
                imported_name: ImportedName::Named("get".to_string()),
                local_name: "get".to_string(),
                is_type_only: false,
                is_type_only_star: false,
                from_style: false,
                span: oxc_span::Span::new(0, 10),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::NpmPackage("lodash".to_string()),
        }],
        ..Default::default()
    }];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
    let root = Path::new("/project");

    let trace = trace_dependency(
        &graph,
        root,
        &[],
        &fallow_config::IgnorePatternSet::empty(),
        "lodash",
        &DependencyCredit {
            script_used_packages: &FxHashSet::default(),
            peer_seeds: &PeerClosureSeeds::default(),
        },
    );
    assert!(trace.is_used);
    assert!(!trace.used_in_scripts);
    assert_eq!(trace.import_count, 1);
    assert_eq!(trace.imported_by[0], PathBuf::from("src/app.ts"));
}

#[test]
fn trace_dependency_unused() {
    let files = vec![DiscoveredFile {
        id: FileId(0),
        path: PathBuf::from("/project/src/app.ts"),
        size_bytes: 100,
    }];
    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/src/app.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];
    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/src/app.ts"),
        ..Default::default()
    }];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
    let root = Path::new("/project");

    let trace = trace_dependency(
        &graph,
        root,
        &[],
        &fallow_config::IgnorePatternSet::empty(),
        "nonexistent-pkg",
        &DependencyCredit {
            script_used_packages: &FxHashSet::default(),
            peer_seeds: &PeerClosureSeeds::default(),
        },
    );
    assert!(!trace.is_used);
    assert!(!trace.used_in_scripts);
    assert_eq!(trace.import_count, 0);
    assert!(trace.imported_by.is_empty());
}

#[test]
fn trace_dependency_used_only_in_scripts() {
    let files = vec![DiscoveredFile {
        id: FileId(0),
        path: PathBuf::from("/project/src/app.ts"),
        size_bytes: 100,
    }];
    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/src/app.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];
    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/src/app.ts"),
        ..Default::default()
    }];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
    let root = Path::new("/project");
    let mut script_used = FxHashSet::default();
    script_used.insert("microbundle".to_string());

    let trace = trace_dependency(
        &graph,
        root,
        &[],
        &fallow_config::IgnorePatternSet::empty(),
        "microbundle",
        &DependencyCredit {
            script_used_packages: &script_used,
            peer_seeds: &PeerClosureSeeds::default(),
        },
    );
    assert!(
        trace.is_used,
        "is_used must be true when the package is referenced from package.json scripts"
    );
    assert!(trace.used_in_scripts);
    assert_eq!(trace.import_count, 0);
    assert!(trace.imported_by.is_empty());
}
