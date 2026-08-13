#![expect(
    clippy::expect_used,
    reason = "benches use unwrap and expect to keep fixture setup concise"
)]
#![allow(
    clippy::significant_drop_tightening,
    reason = "the external Criterion macro owns the benchmark lifecycle"
)]

use std::path::{Path, PathBuf};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use fallow_config::WorkspaceInfo;
use fallow_graph::project::ProjectState;
use fallow_types::discover::{DiscoveredFile, FileId};

const FILE_COUNT: usize = 2_000;
const WORKSPACE_COUNT: usize = 20;

struct GraphFixture {
    root: PathBuf,
    files: Vec<DiscoveredFile>,
    workspaces: Vec<WorkspaceInfo>,
    lookup_paths: Vec<PathBuf>,
}

fn create_graph_fixture() -> GraphFixture {
    let root = PathBuf::from("/bench/project");
    let workspaces = (0..WORKSPACE_COUNT)
        .map(|index| WorkspaceInfo {
            root: root.join(format!("packages/pkg-{index}")),
            name: format!("@bench/pkg-{index}"),
            is_internal_dependency: false,
        })
        .collect::<Vec<_>>();
    let files = (0..FILE_COUNT)
        .map(|index| {
            let workspace = index % WORKSPACE_COUNT;
            let path = root.join(format!("packages/pkg-{workspace}/src/module-{index}.ts"));
            DiscoveredFile {
                id: FileId(u32::try_from(index).expect("fixture file count fits in u32")),
                size_bytes: 256,
                path,
            }
        })
        .collect::<Vec<_>>();
    let lookup_paths = files
        .iter()
        .step_by(17)
        .map(|file| file.path.clone())
        .collect();
    GraphFixture {
        root,
        files,
        workspaces,
        lookup_paths,
    }
}

fn component_graph_project_state_build(c: &mut Criterion) {
    c.bench_function("component_graph_project_state_build", |bencher| {
        bencher.iter_batched(
            create_graph_fixture,
            |fixture| ProjectState::new(fixture.files, fixture.workspaces),
            BatchSize::LargeInput,
        );
    });
}

fn component_graph_project_state_lookups(c: &mut Criterion) {
    c.bench_function("component_graph_project_state_lookups", |bencher| {
        bencher.iter_batched_ref(
            create_graph_fixture,
            |fixture| {
                let state = ProjectState::new(fixture.files.clone(), fixture.workspaces.clone());
                let mut hits = 0usize;
                for path in &fixture.lookup_paths {
                    if let Some(id) = state.id_for_path(path) {
                        hits += usize::from(state.stable_key_for_file(&fixture.root, id).is_some());
                    }
                }
                hits
            },
            BatchSize::LargeInput,
        );
    });
}

fn component_graph_project_state_workspace_queries(c: &mut Criterion) {
    c.bench_function(
        "component_graph_project_state_workspace_queries",
        |bencher| {
            bencher.iter_batched_ref(
                create_graph_fixture,
                |fixture| {
                    let state =
                        ProjectState::new(fixture.files.clone(), fixture.workspaces.clone());
                    let mut total = 0usize;
                    for workspace in state.workspaces() {
                        total += state.files_in_workspace(workspace).len();
                        total += usize::from(state.workspace_by_name(&workspace.name).is_some());
                    }
                    total += usize::from(
                        state
                            .id_for_path(Path::new("/bench/project/packages/missing/src/nope.ts"))
                            .is_none(),
                    );
                    total
                },
                BatchSize::LargeInput,
            );
        },
    );
}

criterion_group!(
    benches,
    component_graph_project_state_build,
    component_graph_project_state_lookups,
    component_graph_project_state_workspace_queries
);
criterion_main!(benches);
