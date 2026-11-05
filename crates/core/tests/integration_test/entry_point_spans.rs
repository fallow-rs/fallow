//! Attribution gate for the entry-point discovery stage.
//!
//! `PipelineTimings::entry_points_ms` alone is a single opaque number, so a
//! slow discovery stage can only be guessed at. These tests assert that the
//! sub-spans are a real partition of that stage: every span is an elapsed time
//! rather than a difference, together they never exceed the stage they
//! subdivide, and a section that did not run does not absorb the work of the
//! section next to it.
//!
//! None of these assertions is a duration threshold. A span's magnitude is a
//! property of the machine, so the invariants here are shape (non-negative,
//! contained in the parent stage) and relative attribution (a skipped section
//! against a sibling that provably did filesystem work in the same run).

use fallow_types::trace::EntryPointSpans;

use super::common::{create_config, fixture_path};

fn spans_total(spans: EntryPointSpans) -> f64 {
    spans.root_ms
        + spans.workspaces_ms
        + spans.plugins_ms
        + spans.infrastructure_ms
        + spans.dynamic_ms
        + spans.dedup_ms
}

#[test]
fn entry_point_spans_partition_the_stage_they_subdivide() {
    let config = create_config(fixture_path("workspace-project"));
    let output = fallow_core::analyze_with_trace(&config).expect("workspace analysis");
    let timings = output.timings.expect("trace timings retained");
    let spans = timings.entry_point_spans;

    // The stage is only worth subdividing on a run that actually discovered
    // something. Anchor that on the discovery result rather than on a duration:
    // the fixture's root package.json contributes entry points every run, on
    // any machine.
    let summary = output
        .results
        .entry_point_summary
        .expect("entry-point summary retained");
    assert!(
        summary
            .by_source
            .iter()
            .any(|(source, count)| source == "package.json" && *count > 0),
        "root discovery must have produced package.json entry points, got {:?}",
        summary.by_source
    );

    for (name, value) in [
        ("root", spans.root_ms),
        ("workspaces", spans.workspaces_ms),
        ("plugins", spans.plugins_ms),
        ("infrastructure", spans.infrastructure_ms),
        ("dynamic", spans.dynamic_ms),
        ("dedup", spans.dedup_ms),
    ] {
        assert!(
            value >= 0.0 && value.is_finite(),
            "{name} span must be an elapsed time, got {value}ms"
        );
    }

    assert!(
        spans_total(spans) <= timings.entry_points_ms,
        "sub-spans must partition the stage: {} sub-span ms vs {} stage ms",
        spans_total(spans),
        timings.entry_points_ms
    );
}

/// Plugin glob work dominates the stage on every real project measured, so the
/// report has to say which half of it paid: compiling the pattern set once, or
/// matching it against every discovered file.
///
/// The workspace fixture activates no plugins, so its glob set is empty and the
/// matching loop is skipped entirely. Asserting a positive match time here
/// measured nothing but the gap between two adjacent clock reads, and failed
/// whenever they landed in the same tick. What this fixture can prove is that
/// the two sub-spans stay inside the stage they subdivide and that neither one
/// borrows time it did not spend.
#[test]
fn plugin_glob_work_is_split_into_compile_and_match() {
    let config = create_config(fixture_path("workspace-project"));
    let output = fallow_core::analyze_with_trace(&config).expect("workspace analysis");
    let spans = output
        .timings
        .expect("trace timings retained")
        .entry_point_spans;

    assert!(
        spans.plugin_glob_build_ms >= 0.0 && spans.plugin_glob_match_ms >= 0.0,
        "sub-spans are elapsed times, not differences: {} compile, {} match",
        spans.plugin_glob_build_ms,
        spans.plugin_glob_match_ms
    );
    assert!(
        spans.plugin_glob_build_ms + spans.plugin_glob_match_ms <= spans.plugins_ms,
        "compile plus match must fit inside the plugin span: {} + {} vs {}",
        spans.plugin_glob_build_ms,
        spans.plugin_glob_match_ms,
        spans.plugins_ms
    );
}

/// Files in the plugin-glob project. Enough that matching the compiled set
/// against them is real work rather than clock noise.
const PLUGIN_GLOB_PROJECT_FILES: usize = 300;

/// The split only means something on a project whose plugins contribute globs:
/// there, matching scales with files times patterns and is the half that grows.
/// A `vitest.config.ts` activates the test-runner plugin from the file alone,
/// so the project needs no installed dependencies to carry a real glob set.
#[test]
fn plugin_glob_match_time_is_reported_when_a_plugin_contributes_globs() {
    let project = tempfile::tempdir().expect("create project");
    let root = project.path();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"plugin-glob-project","version":"1.0.0"}"#,
    )
    .expect("write package.json");
    std::fs::write(
        root.join("vitest.config.ts"),
        "export default { test: { include: ['src/**/*.test.ts'] } };\n",
    )
    .expect("write vitest config");
    let src = root.join("src");
    std::fs::create_dir_all(&src).expect("create src");
    for index in 0..PLUGIN_GLOB_PROJECT_FILES {
        std::fs::write(
            src.join(format!("module{index}.ts")),
            format!("export const value{index} = {index};\n"),
        )
        .expect("write module");
    }

    let config = create_config(root.to_path_buf());
    let output = fallow_core::analyze_with_trace(&config).expect("plugin glob analysis");
    let spans = output
        .timings
        .expect("trace timings retained")
        .entry_point_spans;

    assert!(
        spans.plugin_glob_match_ms > 0.0,
        "matching {PLUGIN_GLOB_PROJECT_FILES} files against an active plugin glob set is timeable \
         work, got {}ms",
        spans.plugin_glob_match_ms
    );
    assert!(
        spans.plugin_glob_build_ms + spans.plugin_glob_match_ms <= spans.plugins_ms,
        "compile plus match must fit inside the plugin span: {} + {} vs {}",
        spans.plugin_glob_build_ms,
        spans.plugin_glob_match_ms,
        spans.plugins_ms
    );
}

/// Consecutive `split_ms` calls carve the stage into adjacent spans, so a
/// misplaced split does not lose time: it moves a neighbour's work into the
/// wrong span. The skipped dynamic branch is where that would show, because
/// with no globs configured its span brackets no work at all.
///
/// The control is the span immediately before it. Infrastructure discovery
/// stats its candidate config directories, lists the project root, and tries to
/// read the root manifests, so it makes real filesystem syscalls on every run
/// and on every machine. A branch that ran nothing cannot cost as much as one
/// that made syscalls; if the dynamic split ever swallowed the section next to
/// it, that ordering inverts. An absolute ceiling would not catch it: removing
/// the infrastructure split moves roughly a tenth of a millisecond into this
/// span, which the previous `< 1.0` assertion accepted.
#[test]
fn skipped_dynamic_discovery_does_not_absorb_the_section_before_it() {
    let config = create_config(fixture_path("workspace-project"));
    assert!(
        config.dynamically_loaded.is_empty(),
        "fixture must not configure dynamically-loaded globs"
    );

    let output = fallow_core::analyze_with_trace(&config).expect("workspace analysis");
    let spans = output
        .timings
        .expect("trace timings retained")
        .entry_point_spans;

    assert!(
        spans.dynamic_ms < spans.infrastructure_ms,
        "the skipped dynamic section brackets no work, yet it is reported as \
         costing at least as much as the infrastructure section that made \
         filesystem calls: {} dynamic ms vs {} infrastructure ms",
        spans.dynamic_ms,
        spans.infrastructure_ms
    );
}
