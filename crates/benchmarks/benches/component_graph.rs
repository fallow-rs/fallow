#![expect(
    clippy::expect_used,
    reason = "benches use unwrap and expect to keep fixture setup concise"
)]
#![allow(
    clippy::significant_drop_tightening,
    reason = "the external Criterion macro owns the benchmark lifecycle"
)]

use std::hint::black_box;
use std::path::PathBuf;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use fallow_config::WorkspaceInfo;
use fallow_graph::project::ProjectState;
use fallow_types::discover::{DiscoveredFile, FileId};

const FILE_COUNT: usize = 2_000;
const WORKSPACE_COUNT: usize = 20;

struct GraphFixture {
    files: Vec<DiscoveredFile>,
    workspaces: Vec<WorkspaceInfo>,
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
    GraphFixture { files, workspaces }
}

fn component_graph_project_state_build(c: &mut Criterion) {
    c.bench_function("component_graph_project_state_build", |bencher| {
        bencher.iter_batched(
            create_graph_fixture,
            |fixture| {
                // Construction is a move, so the routine also reads the state
                // the way analysis setup does. Without the reads the timed
                // body is empty and Criterion reports zero time.
                let state = ProjectState::new(fixture.files, fixture.workspaces);
                let bytes: u64 = state.files().iter().map(|file| file.size_bytes).sum();
                (black_box(bytes), state.workspaces().len())
            },
            BatchSize::LargeInput,
        );
    });
}

criterion_group!(benches, component_graph_project_state_build);
criterion_main!(benches);
