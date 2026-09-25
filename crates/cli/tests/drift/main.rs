#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed fixture step must abort the case with its message"
)]

//! Differential drift harness.
//!
//! Generates small projects, runs them through the CLI, the MCP server and
//! `fallow_api` in-process, and checks the invariants that
//! `docs/development/drift-contract.md` marks as checked by the harness.
//!
//! - `FALLOW_DRIFT_CASES`: cases per invariant (default [`DEFAULT_CASES`]).
//! - `FALLOW_DRIFT_SEED`: a number, or `random`. The default is a fixed seed.
//!
//! The harness needs the `fallow-mcp` binary next to the `fallow` binary:
//! run `cargo build -p fallow-mcp` before `cargo test -p fallow-cli --test drift`.

#[path = "../common/mod.rs"]
mod common;
mod invariants;
mod keys;
mod model;
mod surfaces;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use proptest::test_runner::{
    Config, FileFailurePersistence, RngSeed, TestCaseError, TestError, TestRunner,
};
use serde_json::Value;
use tempfile::TempDir;

use crate::invariants::{ExitRule, Verdict, VerdictRuns};
use crate::keys::{AuditKeys, FindingKey, KeySet, audit_keys, combined_keys, envelope_keys};
use crate::model::{Materialized, ProjectModel, SELECTED_WORKSPACE, project_strategy};
use crate::surfaces::{
    Analysis, McpPath, McpServer, Scope, api_audit, api_dead_code_keys_with_baseline, api_keys,
    cli_audit, cli_combined, cli_envelope, cli_human_verdict_code, cli_keys, cli_save_baseline,
    cli_verdict_envelope, mcp_audit, mcp_bin, mcp_envelope, mcp_keys, mcp_supports, run_cli,
    run_cli_format,
};

/// Cases per invariant when `FALLOW_DRIFT_CASES` is unset. Small, so the
/// blocking PR job stays within a few minutes.
const DEFAULT_CASES: u32 = 6;
/// The fixed seed of the blocking PR job.
const DEFAULT_SEED: u64 = 0x00fa_1107_d21f;
/// Shrink steps before the harness reports the smallest case found so far.
const MAX_SHRINK_ITERS: u32 = 96;
/// The base commit of every generated project, as seen from its head commit.
const BASE_REF: &str = "HEAD~1";
/// Failing seeds land here and replay first on the next run.
const REGRESSIONS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/drift/drift.proptest-regressions"
);

fn case_count() -> u32 {
    std::env::var("FALLOW_DRIFT_CASES").map_or(DEFAULT_CASES, |value| {
        // Zero cases would pass every invariant without a run.
        match value.parse::<u32>() {
            Ok(0) | Err(_) => {
                panic!("FALLOW_DRIFT_CASES must be a positive number, got {value:?}")
            }
            Ok(cases) => cases,
        }
    })
}

fn seed() -> u64 {
    match std::env::var("FALLOW_DRIFT_SEED").as_deref() {
        Err(_) => DEFAULT_SEED,
        Ok("random") => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(DEFAULT_SEED, |elapsed| elapsed.as_nanos() as u64),
        Ok(value) => value.parse().unwrap_or_else(|_| {
            panic!("FALLOW_DRIFT_SEED must be a number or `random`, got {value:?}")
        }),
    }
}

/// Run `check` over generated projects and panic with the shrunk case on failure.
fn run_invariant(name: &str, check: impl Fn(&ProjectModel) -> Verdict) {
    mcp_bin();
    let cases = case_count();
    let seed = seed();
    let config = Config {
        cases,
        max_shrink_iters: MAX_SHRINK_ITERS,
        rng_seed: RngSeed::Fixed(seed),
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(REGRESSIONS))),
        ..Config::default()
    };
    let mut runner = TestRunner::new(config);
    let outcome = runner.run(&project_strategy(), |model| {
        check(&model).map_err(TestCaseError::fail)
    });
    match outcome {
        Ok(()) => {}
        Err(TestError::Fail(reason, model)) => panic!(
            "{name} failed (FALLOW_DRIFT_SEED={seed}, FALLOW_DRIFT_CASES={cases}).\n\
             Shrunk case:\n{reason}\n\nModel: {model:#?}"
        ),
        Err(TestError::Abort(reason)) => {
            panic!("{name} aborted (FALLOW_DRIFT_SEED={seed}): {reason}")
        }
    }
}

/// One materialized project in its own temporary directory.
struct Project {
    _dir: TempDir,
    root: PathBuf,
    scratch: PathBuf,
    files: Materialized,
}

impl Project {
    fn new(model: &ProjectModel, suppressions: bool) -> Self {
        Self::from_files(model.materialize(suppressions))
    }

    /// A project from rendered files, for a fixed case that needs a file the
    /// generator does not write, such as a config file.
    fn from_files(files: Materialized) -> Self {
        let dir = tempfile::tempdir().expect("create case dir");
        let base = dunce::canonicalize(dir.path()).expect("canonicalize case dir");
        let root = base.join("project");
        let scratch = base.join("scratch");
        std::fs::create_dir_all(&root).expect("create project dir");
        std::fs::create_dir_all(&scratch).expect("create scratch dir");
        model::write_repository(&root, &files);
        Self {
            _dir: dir,
            root,
            scratch,
            files,
        }
    }

    /// The files that changed between the base and the head commit, as git
    /// reports them for `--changed-since`, relative to the project root.
    fn changed_files(&self) -> Vec<String> {
        model::git(
            &self.root,
            &["diff", "--name-only", &format!("{BASE_REF}...HEAD")],
        )
        .lines()
        .map(str::to_string)
        .collect()
    }

    /// The renames between the base and the head commit, as git detects them
    /// for `fallow audit`: the same `diff --name-status --find-renames` over
    /// the committed range, so the default similarity threshold applies. A
    /// model rename with an edit that drops the similarity below the
    /// threshold is a delete plus an add here, as it is for audit.
    fn git_renames(&self) -> Vec<(String, String)> {
        let output = model::git(
            &self.root,
            &[
                "diff",
                "--name-status",
                "-z",
                "--find-renames",
                "--end-of-options",
                &format!("{BASE_REF}...HEAD"),
            ],
        );
        let mut fields = output.split('\0').filter(|field| !field.is_empty());
        let mut renames = Vec::new();
        while let Some(status) = fields.next() {
            let Some(first) = fields.next() else {
                break;
            };
            if status.starts_with('R') || status.starts_with('C') {
                let Some(second) = fields.next() else {
                    break;
                };
                if status.starts_with('R') {
                    renames.push((first.to_string(), second.to_string()));
                }
            }
        }
        renames
    }

    /// The added line ranges of each head file, inclusive, from the same
    /// zero-context diff against the base commit that audit reads for the
    /// clone-group demotion.
    fn added_lines(&self) -> BTreeMap<String, Vec<(u64, u64)>> {
        let diff = model::git(
            &self.root,
            &[
                "diff",
                "--relative",
                "--unified=0",
                "--end-of-options",
                BASE_REF,
            ],
        );
        let mut added: BTreeMap<String, Vec<(u64, u64)>> = BTreeMap::new();
        let mut file: Option<String> = None;
        for line in diff.lines() {
            if let Some(path) = line.strip_prefix("+++ ") {
                file = path.strip_prefix("b/").map(str::to_string);
                continue;
            }
            let (Some(path), Some(hunk)) = (&file, line.strip_prefix("@@ ")) else {
                continue;
            };
            let Some(new_side) = hunk.split(' ').find_map(|field| field.strip_prefix('+')) else {
                continue;
            };
            let (start, count) = new_side.split_once(',').unwrap_or((new_side, "1"));
            let (Ok(start), Ok(count)) = (start.parse::<u64>(), count.parse::<u64>()) else {
                continue;
            };
            if count > 0 {
                added
                    .entry(path.clone())
                    .or_default()
                    .push((start, start + count - 1));
            }
        }
        added
    }

    /// Prefix a failure with the files of the case, so the report stands alone.
    fn explain(&self, verdict: Verdict) -> Verdict {
        verdict.map_err(|err| {
            format!(
                "{err}\n\nProject (head commit):\n{}",
                model::describe(&self.files)
            )
        })
    }
}

thread_local! {
    static SERVER: RefCell<Option<McpServer>> = const { RefCell::new(None) };
    static TYPED_SERVER: RefCell<Option<McpServer>> = const { RefCell::new(None) };
}

/// Run `f` with this test thread's MCP server, and start it on first use.
fn with_server<T>(f: impl FnOnce(&mut McpServer) -> T) -> T {
    SERVER.with(|cell| {
        let mut slot = cell.borrow_mut();
        let server = slot.get_or_insert_with(McpServer::start);
        f(server)
    })
}

/// Run `f` with this test thread's typed-only MCP server (see
/// [`McpServer::start_typed_only`]), and start it on first use.
fn with_typed_server<T>(f: impl FnOnce(&mut McpServer) -> T) -> T {
    TYPED_SERVER.with(|cell| {
        let mut slot = cell.borrow_mut();
        let server = slot.get_or_insert_with(McpServer::start_typed_only);
        f(server)
    })
}

/// Every surface for one analysis under one scope, labelled for a diff.
fn all_surfaces(analysis: Analysis, project: &Project, scope: &Scope) -> Vec<(String, KeySet)> {
    let root = &project.root;
    let mut results = vec![
        ("CLI".to_string(), cli_keys(analysis, root, scope, None)),
        ("fallow_api".to_string(), api_keys(analysis, root, scope)),
    ];
    if mcp_supports(analysis, scope) {
        for path in [McpPath::Typed, McpPath::CliFallback] {
            let keys = with_server(|server| {
                mcp_keys(server, path, analysis, root, scope, &project.scratch)
            });
            results.push((format!("MCP {path:?}"), keys));
        }
    }
    results
}

#[test]
#[ignore = "needs the fallow-mcp binary; run with: cargo build -p fallow-mcp && cargo test -p fallow-cli --test drift -- --include-ignored"]
fn i1_check_output_equals_dead_code_output() {
    run_invariant("I1", |model| {
        let project = Project::new(model, true);
        let check = run_cli(&project.root, &["check".to_string()]);
        let dead_code = run_cli(&project.root, &["dead-code".to_string()]);
        cli_envelope(&dead_code);
        project.explain(invariants::i1_alias_identical(&check, &dead_code))
    });
}

#[test]
#[ignore = "needs the fallow-mcp binary; run with: cargo build -p fallow-mcp && cargo test -p fallow-cli --test drift -- --include-ignored"]
fn i2_finding_sets_agree_across_surfaces() {
    run_invariant("I2", |model| {
        let project = Project::new(model, true);
        for analysis in Analysis::ALL {
            let results = all_surfaces(analysis, &project, &Scope::default());
            project.explain(invariants::surfaces_agree(
                &format!("{analysis:?} finding sets differ"),
                &results,
            ))?;
        }
        Ok(())
    });
}

#[test]
#[ignore = "needs the fallow-mcp binary; run with: cargo build -p fallow-mcp && cargo test -p fallow-cli --test drift -- --include-ignored"]
fn i4_audit_splits_the_changed_head_findings() {
    run_invariant("I4", |model| {
        let project = Project::new(model, true);
        let audit = audit_keys(&cli_audit(&project.root));
        project.explain(invariants::i4_audit_attribution(
            &expected_audit_split(&project),
            &audit,
        ))
    });
}

#[test]
#[ignore = "needs the fallow-mcp binary; run with: cargo build -p fallow-mcp && cargo test -p fallow-cli --test drift -- --include-ignored"]
fn i5_audit_agrees_across_surfaces() {
    run_invariant("I5", |model| {
        let project = Project::new(model, true);
        let results = vec![
            ("CLI".to_string(), audit_keys(&cli_audit(&project.root))),
            (
                "MCP Typed".to_string(),
                audit_keys(&with_typed_server(|server| {
                    mcp_audit(server, &project.root)
                })),
            ),
            (
                "fallow_api".to_string(),
                audit_keys(&api_audit(&project.root)),
            ),
        ];
        project.explain(invariants::i5_audit_surfaces_agree(&results))
    });
}

/// A location rule of one I8 scope: whether a finding path is in the scope.
type InScope = Box<dyn Fn(&str) -> bool>;

/// The scopes I8 checks for one project, each with its location rule.
fn scopes(model: &ProjectModel, project: &Project) -> Vec<(Scope, Option<InScope>)> {
    let changed = project.changed_files();
    let mut scopes: Vec<(Scope, Option<InScope>)> = vec![
        (
            Scope {
                changed_since: Some(BASE_REF.to_string()),
                ..Scope::default()
            },
            Some(Box::new(move |path: &str| {
                changed.iter().any(|file| file == path)
            })),
        ),
        (
            Scope {
                production: true,
                ..Scope::default()
            },
            None,
        ),
    ];
    if model.has_workspaces() {
        scopes.push((
            Scope {
                workspace: Some(SELECTED_WORKSPACE.to_string()),
                ..Scope::default()
            },
            Some(Box::new(|path: &str| path.starts_with("packages/a/"))),
        ));
    }
    scopes
}

#[test]
#[ignore = "needs the fallow-mcp binary; run with: cargo build -p fallow-mcp && cargo test -p fallow-cli --test drift -- --include-ignored"]
fn i8_scope_flags_narrow_the_same_way_on_every_surface() {
    run_invariant("I8", |model| {
        let project = Project::new(model, true);
        for analysis in Analysis::ALL {
            let unscoped = cli_keys(analysis, &project.root, &Scope::default(), None);
            for (scope, in_scope) in scopes(model, &project) {
                let context = format!("{analysis:?} with {scope:?}");
                let results = all_surfaces(analysis, &project, &scope);
                project.explain(invariants::surfaces_agree(&context, &results))?;
                if let Some(in_scope) = in_scope {
                    let scoped = &results[0].1;
                    project.explain(invariants::i8_narrows(&context, scoped, &unscoped))?;
                    let exempt = if scope.changed_since.is_some() {
                        invariants::CHANGED_SINCE_UNFILTERED_KINDS
                    } else {
                        &[]
                    };
                    project.explain(invariants::i8_inside_scope(
                        &context, scoped, exempt, in_scope,
                    ))?;
                }
            }
        }
        Ok(())
    });
}

/// Positive control of I8. A fixed workspace project holds one clone group
/// with an instance in `pkg-a` and an instance in `pkg-b`. With
/// `--workspace pkg-a`, every surface keeps that group whole. Without this
/// control, a generator that never puts a clone across two packages passes I8
/// without a real check.
#[test]
#[ignore = "needs the fallow-mcp binary; run with: cargo build -p fallow-mcp && cargo test -p fallow-cli --test drift -- --include-ignored"]
fn i8_control_keeps_a_clone_group_across_workspaces() {
    use crate::model::{ExportSpec, FileSpec};
    mcp_bin();
    let file = |second_package: bool| FileSpec {
        second_package,
        entry_imported: true,
        suppress_file: false,
        exports: vec![ExportSpec {
            is_type: false,
            suppressed: false,
        }],
        imports: Vec::new(),
    };
    let model = ProjectModel {
        workspaces: true,
        files: vec![file(false), file(true)],
        deps: Vec::new(),
        duplicate: Some((0, 1, false)),
        complex: None,
        changes: Vec::new(),
        baseline_mask: vec![true],
    };
    let project = Project::new(&model, true);
    let scope = Scope {
        workspace: Some(SELECTED_WORKSPACE.to_string()),
        ..Scope::default()
    };
    let results = all_surfaces(Analysis::Dupes, &project, &scope);
    project
        .explain(invariants::surfaces_agree(
            "Dupes with --workspace",
            &results,
        ))
        .unwrap_or_else(|err| panic!("{err}"));
    let across = results[0].1.iter().any(|key| {
        key.kind == keys::DUPLICATION_KIND
            && key.path.contains("packages/a/")
            && key.path.contains("packages/b/")
    });
    assert!(
        across,
        "`--workspace {SELECTED_WORKSPACE}` must keep the clone group across both packages:\n{}",
        keys::render(&results[0].1)
    );
}

/// Positive control of I4 and I5. On a fixed project, the expected split and
/// every audit surface see a finding that moved with a renamed file and the
/// dependency findings of a changed manifest. Without this control, a
/// generator that never renames a file with a finding, or never touches a
/// manifest, passes I4 and I5 without a real check.
#[test]
#[ignore = "needs the fallow-mcp binary; run with: cargo build -p fallow-mcp && cargo test -p fallow-cli --test drift -- --include-ignored"]
fn audit_controls_see_renames_and_manifests() {
    let project = Project::new(&audit_control_model(true), true);
    assert_eq!(
        project.git_renames(),
        vec![("src/f0.ts".to_string(), "src/r0.ts".to_string())],
        "git must detect the rename of the control project"
    );
    let expected = expected_audit_split(&project);
    for (split, keys, kind, path, symbol) in [
        (
            "inherited",
            &expected.inherited,
            "unused_exports",
            "src/r0.ts",
            "e0x0",
        ),
        (
            "inherited",
            &expected.inherited,
            "unused_dependencies",
            "package.json",
            "dep-0",
        ),
        (
            "introduced",
            &expected.introduced,
            "unused_dependencies",
            "package.json",
            "dep-added",
        ),
    ] {
        assert!(
            keys.iter()
                .any(|key| key.kind == kind && key.path == path && key.symbol == symbol),
            "the expected split has no {split} {kind} {path} {symbol}\n{}",
            keys::render(keys)
        );
    }
    let cli = audit_keys(&cli_audit(&project.root));
    project
        .explain(invariants::i4_audit_attribution(&expected, &cli))
        .unwrap_or_else(|err| panic!("{err}"));
    let results = vec![
        ("CLI".to_string(), cli),
        (
            "MCP Typed".to_string(),
            audit_keys(&with_typed_server(|server| {
                mcp_audit(server, &project.root)
            })),
        ),
        (
            "fallow_api".to_string(),
            audit_keys(&api_audit(&project.root)),
        ),
    ];
    project
        .explain(invariants::i5_audit_surfaces_agree(&results))
        .unwrap_or_else(|err| panic!("{err}"));

    let unchanged = Project::new(&audit_control_model(false), true);
    let is_dependency = |key: &FindingKey| key.kind == "unused_dependencies";
    assert!(
        cli_keys(Analysis::DeadCode, &unchanged.root, &Scope::default(), None)
            .iter()
            .any(is_dependency),
        "the control project has no unused dependency"
    );
    let results = vec![
        ("CLI".to_string(), audit_keys(&cli_audit(&unchanged.root))),
        (
            "MCP Typed".to_string(),
            audit_keys(&with_typed_server(|server| {
                mcp_audit(server, &unchanged.root)
            })),
        ),
        (
            "fallow_api".to_string(),
            audit_keys(&api_audit(&unchanged.root)),
        ),
    ];
    for (surface, audit) in &results {
        let reported: KeySet = audit
            .introduced
            .union(&audit.inherited)
            .filter(|key| is_dependency(key))
            .cloned()
            .collect();
        assert!(
            reported.is_empty(),
            "{surface} audit reported dependency findings of a manifest that did not change\n{}",
            keys::render(&reported)
        );
    }
    unchanged
        .explain(invariants::i5_audit_surfaces_agree(&results))
        .unwrap_or_else(|err| panic!("{err}"));
}

/// Positive control of the clone-group rules of I4. A clone that grows with
/// an added line in each instance is a new clone group, so audit reports it
/// as introduced. A clone that changes shape only because a deleted file took
/// an import away holds no added line, so audit demotes it to inherited
/// (#2164). Without this control, an identity that ignores the clone size, or
/// an expected split without the demotion, passes I4 until a random case
/// finds the gap.
#[test]
#[ignore = "needs the fallow-mcp binary; run with: cargo build -p fallow-mcp && cargo test -p fallow-cli --test drift -- --include-ignored"]
fn audit_controls_see_clone_group_changes() {
    use crate::model::{ChangeSpec, ExportSpec, FileSpec};
    let file = |exports: usize, imports: Vec<(usize, usize)>| FileSpec {
        second_package: false,
        entry_imported: false,
        suppress_file: false,
        exports: vec![
            ExportSpec {
                is_type: false,
                suppressed: false,
            };
            exports
        ],
        imports,
    };
    let model = |files: Vec<FileSpec>, duplicate: (usize, usize), changes| ProjectModel {
        workspaces: false,
        files,
        deps: Vec::new(),
        duplicate: Some((duplicate.0, duplicate.1, false)),
        complex: None,
        changes,
        baseline_mask: vec![false],
    };
    let grown = model(
        vec![file(0, Vec::new()), file(0, Vec::new())],
        (0, 1),
        vec![ChangeSpec::Edit(0), ChangeSpec::Edit(1)],
    );
    let reshaped = model(
        vec![
            file(1, Vec::new()),
            file(1, Vec::new()),
            file(0, vec![(0, 0)]),
        ],
        (1, 2),
        vec![ChangeSpec::Delete(0)],
    );
    // The deleted file was also imported, so the head edits an import line
    // inside the surviving clone. Its text changes, so the group is new.
    let edited = model(
        vec![
            file(1, Vec::new()),
            file(1, Vec::new()),
            file(0, vec![(0, 0), (1, 0)]),
        ],
        (0, 2),
        vec![ChangeSpec::Delete(1)],
    );
    for (name, model, split) in [
        ("grown", grown, "introduced"),
        ("reshaped", reshaped, "inherited"),
        ("edited", edited, "introduced"),
    ] {
        let project = Project::new(&model, true);
        let expected = expected_audit_split(&project);
        let keys = if split == "introduced" {
            &expected.introduced
        } else {
            &expected.inherited
        };
        assert!(
            keys.iter().any(|key| key.kind == keys::DUPLICATION_KIND),
            "the {name} control has no {split} clone group\n{}",
            keys::render(keys)
        );
        let cli = audit_keys(&cli_audit(&project.root));
        project
            .explain(invariants::i4_audit_attribution(&expected, &cli))
            .unwrap_or_else(|err| panic!("{name} control: {err}"));
    }
}

/// Two stale suppressions in one file are two findings. The base has a stale
/// `unused-export` suppression. The head deletes the importer, so the file is
/// unused, and its `unused-type` suppression is stale too. That one is
/// introduced.
#[test]
#[ignore = "needs the fallow-mcp binary; run with: cargo build -p fallow-mcp && cargo test -p fallow-cli --test drift -- --include-ignored"]
fn audit_controls_see_each_stale_suppression() {
    use crate::model::{ChangeSpec, ExportSpec, FileSpec};
    let suppressed = |is_type: bool| ExportSpec {
        is_type,
        suppressed: true,
    };
    let model = ProjectModel {
        workspaces: false,
        files: vec![
            FileSpec {
                second_package: false,
                entry_imported: true,
                suppress_file: false,
                exports: Vec::new(),
                imports: vec![(1, 0)],
            },
            FileSpec {
                second_package: false,
                entry_imported: false,
                suppress_file: false,
                exports: vec![suppressed(false), suppressed(true)],
                imports: Vec::new(),
            },
        ],
        deps: Vec::new(),
        duplicate: None,
        complex: None,
        changes: vec![ChangeSpec::Edit(1), ChangeSpec::Delete(0)],
        baseline_mask: vec![false],
    };
    let project = Project::new(&model, true);
    let expected = expected_audit_split(&project);
    assert!(
        expected
            .introduced
            .iter()
            .any(|key| key.kind == "stale_suppressions"),
        "the new stale suppression must be introduced\n{}",
        keys::render(&expected.introduced)
    );
    let cli = audit_keys(&cli_audit(&project.root));
    project
        .explain(invariants::i4_audit_attribution(&expected, &cli))
        .unwrap_or_else(|err| panic!("stale suppression control: {err}"));
}

/// One file with two unused exports, entry-imported, and one unused
/// dependency. The head commit renames the file and, with
/// `change_manifest`, adds the unused `dep-added` to the manifest.
fn audit_control_model(change_manifest: bool) -> ProjectModel {
    use crate::model::{ChangeSpec, DepSpec, ExportSpec, FileSpec};
    let export = ExportSpec {
        is_type: false,
        suppressed: false,
    };
    let mut changes = vec![ChangeSpec::Rename(0)];
    if change_manifest {
        changes.push(ChangeSpec::AddDependency);
    }
    ProjectModel {
        workspaces: false,
        files: vec![FileSpec {
            second_package: false,
            entry_imported: true,
            suppress_file: false,
            exports: vec![export.clone(), export],
            imports: Vec::new(),
        }],
        deps: vec![DepSpec {
            used_by: None,
            dev: false,
        }],
        duplicate: None,
        complex: None,
        changes,
        baseline_mask: vec![true],
    }
}

/// The split that I4 expects, built without the audit code: the head
/// findings of the standalone commands in scope, each one introduced when no
/// finding with the same identity exists at the base commit.
///
/// - In scope: a key with a path in a changed file. A dependency finding has
///   its manifest as its path, so it is in scope only when the manifest
///   changed. A key over several files (a clone group) is in scope when one
///   of them changed.
/// - Identity: the kind, the paths, and the symbol, without line numbers. The
///   base paths follow the renames of the head commit first. The renames come
///   from git, not from the model: a rename counts only when git detects it.
/// - A clone group with a new identity is inherited when no instance holds an
///   added line: the change did not write the duplicated text (#2164).
fn expected_audit_split(project: &Project) -> AuditKeys {
    let changed = changed_paths(&project.files);
    let base_root = project.scratch.join("base");
    for (path, content) in &project.files.base {
        let target = base_root.join(path);
        std::fs::create_dir_all(target.parent().expect("base file has a parent"))
            .expect("create base dir");
        std::fs::write(&target, content).expect("write base file");
    }
    let git_renames = project.git_renames();
    let renames: BTreeMap<&str, &str> = git_renames
        .iter()
        .map(|(old, new)| (old.as_str(), new.as_str()))
        .collect();
    let added = project.added_lines();
    let unscoped = Scope::default();
    let base_identities: BTreeSet<Identity> = Analysis::ALL
        .into_iter()
        .flat_map(|analysis| cli_keys(analysis, &base_root, &unscoped, None))
        .map(|key| identity(&key, &renames, &project.files.base))
        .collect();
    let mut expected = AuditKeys::default();
    for key in Analysis::ALL
        .into_iter()
        .flat_map(|analysis| cli_keys(analysis, &project.root, &unscoped, None))
    {
        if !key.path.split(" -> ").any(|path| changed.contains(path)) {
            continue;
        }
        let untouched_clone =
            key.kind == keys::DUPLICATION_KIND && !clone_holds_added_line(&key.symbol, &added);
        if untouched_clone
            || base_identities.contains(&identity(&key, &BTreeMap::new(), &project.files.head))
        {
            expected.inherited.insert(key);
        } else {
            expected.introduced.insert(key);
        }
    }
    expected
}

/// Whether an instance of a clone-group symbol (`path:start-end | ...`)
/// holds an added line of its file.
fn clone_holds_added_line(symbol: &str, added: &BTreeMap<String, Vec<(u64, u64)>>) -> bool {
    symbol.split(" | ").any(|instance| {
        let Some((path, range)) = instance.rsplit_once(':') else {
            return true;
        };
        let Some((start, end)) = range
            .split_once('-')
            .and_then(|(start, end)| Some((start.parse::<u64>().ok()?, end.parse::<u64>().ok()?)))
        else {
            return true;
        };
        added.get(path).is_some_and(|hunks| {
            hunks
                .iter()
                .any(|&(first, last)| first <= end && start <= last)
        })
    })
}

/// Head paths whose content differs from the base commit, and new head paths
/// (renamed or added files).
fn changed_paths(files: &Materialized) -> BTreeSet<String> {
    files
        .head
        .iter()
        .filter(|(path, content)| files.base.get(*path) != Some(*content))
        .map(|(path, _)| path.clone())
        .collect()
}

/// Line-independent identity of a finding: kind, sorted paths after renames,
/// and symbol. A clone group has line ranges in its symbol, so its identity
/// holds the sorted line counts and the text of its instances instead. The
/// audit key of a clone group holds its size and a hash of its text too, so a
/// clone that grows with added lines, or whose text an edit changes, is a new
/// clone group.
type Identity = (String, Vec<String>, String);

fn identity(
    key: &FindingKey,
    renames: &BTreeMap<&str, &str>,
    contents: &BTreeMap<String, String>,
) -> Identity {
    let mut paths: Vec<String> = key
        .path
        .split(" -> ")
        .map(|path| renames.get(path).copied().unwrap_or(path).to_string())
        .collect();
    paths.sort();
    let symbol = if key.kind == keys::DUPLICATION_KIND {
        format!(
            "{}#{}",
            clone_line_counts(&key.symbol),
            clone_texts(&key.symbol, contents)
        )
    } else {
        key.symbol.clone()
    };
    (key.kind.clone(), paths, symbol)
}

/// The sorted source text of the instances in a clone-group symbol
/// (`path:start-end | path:start-end`), read from `contents` by line range.
fn clone_texts(symbol: &str, contents: &BTreeMap<String, String>) -> String {
    let mut texts: Vec<String> = symbol
        .split(" | ")
        .filter_map(|instance| {
            let (path, range) = instance.rsplit_once(':')?;
            let (start, end) = range.split_once('-')?;
            let start = start.parse::<usize>().ok()?.checked_sub(1)?;
            let end = end.parse::<usize>().ok()?;
            let lines: Vec<&str> = contents.get(path)?.lines().collect();
            Some(lines.get(start..end.min(lines.len()))?.join("\n"))
        })
        .collect();
    texts.sort_unstable();
    texts.join("\u{1f}")
}

/// The sorted line counts of the instances in a clone-group symbol
/// (`path:start-end | path:start-end`), without the paths and line positions.
fn clone_line_counts(symbol: &str) -> String {
    let mut counts: Vec<u64> = symbol
        .split(" | ")
        .filter_map(|instance| {
            let (_, range) = instance.rsplit_once(':')?;
            let (start, end) = range.split_once('-')?;
            Some(end.parse::<u64>().ok()? + 1 - start.parse::<u64>().ok()?)
        })
        .collect();
    counts.sort_unstable();
    counts
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

#[test]
#[ignore = "needs the fallow-mcp binary; run with: cargo build -p fallow-mcp && cargo test -p fallow-cli --test drift -- --include-ignored"]
fn i6_suppressions_and_baselines_never_add_findings() {
    run_invariant("I6", |model| {
        let with = Project::new(model, true);
        let without = Project::new(model, false);
        let unscoped = Scope::default();
        for analysis in Analysis::ALL {
            let context = format!("{analysis:?}");
            with.explain(invariants::i6_suppression_never_adds(
                &context,
                &cli_keys(analysis, &with.root, &unscoped, None),
                &cli_keys(analysis, &without.root, &unscoped, None),
            ))?;
            check_baseline_monotonic(analysis, &without, &model.baseline_mask)?;
        }
        let combined =
            |project: &Project| combined_keys(&cli_envelope(&run_cli(&project.root, &[]))).all();
        with.explain(invariants::i6_suppression_never_adds(
            "combined",
            &combined(&with),
            &combined(&without),
        ))
    });
}

fn check_baseline_monotonic(analysis: Analysis, project: &Project, mask: &[bool]) -> Verdict {
    let (full, partial) = save_baselines(analysis, project, mask);
    let unscoped = Scope::default();
    let with_partial = cli_keys(analysis, &project.root, &unscoped, Some(&partial));
    project.explain(invariants::i6_baseline_never_adds(
        &format!("{analysis:?} baseline"),
        &cli_keys(analysis, &project.root, &unscoped, None),
        &with_partial,
        &cli_keys(analysis, &project.root, &unscoped, Some(&full)),
    ))?;
    if analysis == Analysis::DeadCode {
        // `fallow_api` reads a dead-code baseline with the same engine
        // function, so the partial baseline hides the same findings there.
        project.explain(invariants::keys_equal(
            "CLI with the partial baseline",
            &with_partial,
            "fallow_api with the partial baseline",
            &api_dead_code_keys_with_baseline(&project.root, &partial),
        ))?;
    }
    Ok(())
}

/// Save a full baseline of `analysis` and a partial copy that keeps the
/// entries `mask` selects. Returns (full, partial).
fn save_baselines(analysis: Analysis, project: &Project, mask: &[bool]) -> (PathBuf, PathBuf) {
    let full = project
        .scratch
        .join(format!("{}-full.json", analysis.cli_command()));
    let partial = project
        .scratch
        .join(format!("{}-partial.json", analysis.cli_command()));
    cli_save_baseline(analysis, &project.root, &full);
    let saved: Value =
        serde_json::from_str(&std::fs::read_to_string(&full).expect("read saved baseline"))
            .expect("parse saved baseline");
    std::fs::write(&partial, subset_baseline(&saved, mask).to_string())
        .expect("write partial baseline");
    (full, partial)
}

#[test]
#[ignore = "needs the fallow-mcp binary; run with: cargo build -p fallow-mcp && cargo test -p fallow-cli --test drift -- --include-ignored"]
fn i3_combined_sections_equal_standalone_commands() {
    run_invariant("I3", |model| {
        let project = Project::new(model, true);
        project.explain(combined_sections_agree(&project, None))?;
        let partial = Analysis::ALL
            .map(|analysis| save_baselines(analysis, &project, &model.baseline_mask).1);
        project.explain(combined_sections_agree(&project, Some(&partial)))
    });
}

/// Compare each section of bare `fallow` with its standalone command, both
/// with the same baseline of each analysis.
fn combined_sections_agree(project: &Project, baselines: Option<&[PathBuf; 3]>) -> Verdict {
    let sections = combined_keys(&cli_combined(&project.root, baselines));
    let unscoped = Scope::default();
    let rows: Vec<(String, KeySet, KeySet)> = Analysis::ALL
        .into_iter()
        .zip([sections.dead_code, sections.dupes, sections.health])
        .enumerate()
        .map(|(index, (analysis, section))| {
            let baseline = baselines.map(|paths| paths[index].as_path());
            (
                analysis.cli_command().to_string(),
                section,
                cli_keys(analysis, &project.root, &unscoped, baseline),
            )
        })
        .collect();
    invariants::i3_sections_equal_standalone(
        if baselines.is_some() {
            "with partial baselines"
        } else {
            "without baselines"
        },
        &rows,
    )
}

/// Positive control of I3 and I7. On the fixed project, full baselines empty
/// every section of bare `fallow`, and the bare JSON run states a failing
/// verdict that the human run of the same project exits on. Without this
/// control, a run that ignores the combined baseline flags, or a stated
/// verdict that never fails, passes I3 and I7 without a real check.
#[test]
fn combined_controls_see_baselines_and_verdicts() {
    let project = Project::new(&fixed_model(false), true);
    let full = Analysis::ALL.map(|analysis| save_baselines(analysis, &project, &[true]).0);
    let without = combined_keys(&cli_combined(&project.root, None));
    let with = combined_keys(&cli_combined(&project.root, Some(&full)));
    for (analysis, without, with) in [
        (Analysis::DeadCode, &without.dead_code, &with.dead_code),
        (Analysis::Dupes, &without.dupes, &with.dupes),
        (Analysis::Health, &without.health, &with.health),
    ] {
        assert!(
            !without.is_empty() && with.is_empty(),
            "a full {analysis:?} baseline must empty the bare `fallow` section\n{}",
            invariants::diff("no baseline", without, "full baseline", with)
        );
    }
    project
        .explain(combined_sections_agree(&project, Some(&full)))
        .unwrap_or_else(|err| panic!("{err}"));

    let json = run_cli(&project.root, &[]);
    let stated =
        invariants::stated_verdict(&cli_envelope(&json)).expect("bare `fallow` states a verdict");
    assert!(
        stated.failed && !stated.enforced_failure && json.code == 0,
        "the bare JSON run states a failure it does not exit on: {stated:?}, exit {}",
        json.code
    );
    let human = run_cli_format(&project.root, &[], "human");
    assert_eq!(
        human.code, 1,
        "the human run fails on the same project\n{}",
        human.stderr
    );
}

/// The gate that a command of the I7 comparison arms beyond its default rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Arm {
    /// Only the default exit rule of the command.
    Default,
    /// `--fail-on-regression` against a regression baseline of the case.
    Regression,
}

/// One command of the I7 comparison.
struct VerdictCommand {
    args: &'static [&'static str],
    rule: ExitRule,
    /// `dupes` has no default exit rule, so its object can be absent.
    requires_object: bool,
    /// Also compare the `--group-by directory` envelope.
    grouped: bool,
    arm: Arm,
}

const VERDICT_COMMANDS: &[VerdictCommand] = &[
    VerdictCommand {
        args: &["dead-code"],
        rule: ExitRule::Enforced,
        requires_object: true,
        grouped: true,
        arm: Arm::Default,
    },
    VerdictCommand {
        args: &["dupes"],
        rule: ExitRule::Enforced,
        requires_object: false,
        grouped: true,
        arm: Arm::Default,
    },
    VerdictCommand {
        args: &["health"],
        rule: ExitRule::Enforced,
        requires_object: true,
        grouped: true,
        arm: Arm::Default,
    },
    VerdictCommand {
        args: &["security"],
        rule: ExitRule::Enforced,
        requires_object: true,
        grouped: false,
        arm: Arm::Default,
    },
    VerdictCommand {
        args: &["audit", "--base", BASE_REF],
        rule: ExitRule::Enforced,
        requires_object: true,
        grouped: false,
        arm: Arm::Default,
    },
    VerdictCommand {
        args: &[],
        rule: ExitRule::CombinedMachine,
        requires_object: true,
        grouped: true,
        arm: Arm::Default,
    },
    VerdictCommand {
        args: &["security", "--gate", "new", "--changed-since", BASE_REF],
        rule: ExitRule::Enforced,
        requires_object: true,
        grouped: false,
        arm: Arm::Default,
    },
    VerdictCommand {
        args: &["dead-code"],
        rule: ExitRule::Enforced,
        requires_object: true,
        grouped: false,
        arm: Arm::Regression,
    },
    VerdictCommand {
        args: &[],
        rule: ExitRule::CombinedMachine,
        requires_object: true,
        grouped: false,
        arm: Arm::Regression,
    },
];

#[test]
#[ignore = "needs the fallow-mcp binary; run with: cargo build -p fallow-mcp && cargo test -p fallow-cli --test drift -- --include-ignored"]
fn i7_every_envelope_states_the_verdict_of_the_human_run() {
    run_invariant("I7", |model| {
        let project = Project::new(model, true);
        // The first mask bit keeps the counts of the head commit, so the
        // regression gate passes, or sets them to zero, so a finding fails it.
        let keep_counts = model.baseline_mask.first().copied().unwrap_or(true);
        let regression = regression_args(&project, keep_counts);
        for command in VERDICT_COMMANDS {
            let extra = match command.arm {
                Arm::Default => &[][..],
                Arm::Regression => &regression[..],
            };
            project.explain(invariants::i7_verdicts_agree(&verdict_runs(
                &project, command, extra,
            )))?;
        }
        project.explain(mcp_verdicts_agree(&project))
    });
}

/// The flags that arm the regression gate against a regression baseline of
/// `project`. With `keep_counts`, the baseline holds the counts of the head
/// commit. Without it, every count is zero, so any finding fails the gate.
fn regression_args(project: &Project, keep_counts: bool) -> Vec<String> {
    let path = project.scratch.join(if keep_counts {
        "regression-kept.json"
    } else {
        "regression-zero.json"
    });
    if !path.is_file() {
        surfaces::cli_save_regression_baseline(&project.root, &path);
        if !keep_counts {
            let text = std::fs::read_to_string(&path).expect("read regression baseline");
            let mut saved: Value = serde_json::from_str(&text).expect("regression baseline JSON");
            // The counts sit in `check`. Other members, such as the analysis
            // identity, must keep their values.
            let counts = saved["check"]
                .as_object_mut()
                .expect("the regression baseline holds dead-code counts");
            for count in counts.values_mut() {
                if count.is_u64() {
                    *count = Value::from(0);
                }
            }
            std::fs::write(&path, saved.to_string()).expect("write regression baseline");
        }
    }
    vec![
        "--fail-on-regression".to_string(),
        "--regression-baseline".to_string(),
        path.display().to_string(),
    ]
}

/// Run one command with `extra` flags in JSON (and grouped JSON) and in the
/// human format.
fn verdict_runs(
    project: &Project,
    command: &VerdictCommand,
    extra: &[String],
) -> VerdictRuns<'static> {
    let mut args: Vec<String> = command.args.iter().map(ToString::to_string).collect();
    args.extend_from_slice(extra);
    let json = run_cli(&project.root, &args);
    let mut machine = vec![("JSON".to_string(), cli_verdict_envelope(&json), json.code)];
    if command.grouped {
        let mut grouped = args.clone();
        grouped.extend(["--group-by".to_string(), "directory".to_string()]);
        let output = run_cli(&project.root, &grouped);
        machine.push((
            "grouped JSON".to_string(),
            cli_verdict_envelope(&output),
            output.code,
        ));
    }
    VerdictRuns {
        command: command.args.first().copied().unwrap_or("fallow"),
        rule: command.rule,
        requires_object: command.requires_object,
        machine,
        human_code: cli_human_verdict_code(&project.root, &args),
    }
}

/// The MCP tools that wrap the CLI envelope state the verdict of the CLI run.
/// The typed path runs `fallow_api`, which runs no CLI gate and publishes no
/// `gate_outcomes`, so only the CLI-fallback path is compared.
fn mcp_verdicts_agree(project: &Project) -> Verdict {
    let unscoped = Scope::default();
    let mut problems = Vec::new();
    for analysis in Analysis::ALL {
        let cli = cli_envelope(&run_cli(
            &project.root,
            &[analysis.cli_command().to_string()],
        ));
        let mcp = with_server(|server| {
            mcp_envelope(
                server,
                McpPath::CliFallback,
                analysis,
                &project.root,
                &unscoped,
                &project.scratch,
            )
        });
        let (cli_verdict, mcp_verdict) = (
            invariants::stated_verdict(&cli),
            invariants::stated_verdict(&mcp),
        );
        if cli_verdict != mcp_verdict {
            problems.push(format!(
                "{analysis:?}: CLI states {cli_verdict:?} ({}), MCP CliFallback states \
                 {mcp_verdict:?} ({})",
                cli["gate_outcomes"], mcp["gate_outcomes"]
            ));
        }
    }
    if problems.is_empty() {
        return Ok(());
    }
    Err(format!("MCP verdicts differ:\n{}", problems.join("\n")))
}

/// Positive control of I7 for the armed gates. On fixed projects, each gate
/// that a machine run enforces fails: `--fail-on-regression` against a
/// baseline with zero counts, `--fail-on-stale-baseline` against a baseline
/// with entries that suppression comments removed, and `security --gate new`
/// on a head commit that adds a sink. Bare `fallow --format json` exits 1 on
/// the first two, and `security --gate` exits 8. Without this control, a
/// harness that expects exit 0 from every bare machine run, or exit 1 from
/// every failed gate, passes I7 on the generated cases.
#[test]
#[ignore = "needs the fallow-mcp binary; run with: cargo build -p fallow-mcp && cargo test -p fallow-cli --test drift -- --include-ignored"]
fn i7_controls_see_every_armed_gate_fail() {
    let project = Project::new(&fixed_model(false), true);
    let regression = regression_args(&project, false);
    for command in VERDICT_COMMANDS
        .iter()
        .filter(|command| command.arm == Arm::Regression)
    {
        assert_armed_gate_fails(&project, command, &regression, "regression", 1);
    }

    // The same marked findings, once as plain comments and once suppressed:
    // a baseline of the plain copy has entries that the suppressed copy
    // does not match.
    let plain = Project::new(&fixed_model(true), false);
    let (baseline, _) = save_baselines(Analysis::DeadCode, &plain, &[true]);
    let suppressed = Project::new(&fixed_model(true), true);
    let stale = [
        "--baseline".to_string(),
        baseline.display().to_string(),
        "--fail-on-stale-baseline".to_string(),
    ];
    for command in VERDICT_COMMANDS
        .iter()
        .filter(|command| command.arm == Arm::Default && matches!(command.args, ["dead-code"] | []))
    {
        assert_armed_gate_fails(&suppressed, command, &stale, "stale-baseline", 1);
    }

    let mut files = fixed_model(false).materialize(true);
    files.head.insert(
        "src/sink.ts".to_string(),
        "import { exec } from \"node:child_process\";\nexport function run(command: string): void {\n  exec(command);\n}\n"
            .to_string(),
    );
    let sink = Project::from_files(files);
    let security = VERDICT_COMMANDS
        .iter()
        .find(|command| command.args.first() == Some(&"security") && command.args.len() > 1)
        .expect("I7 runs `security --gate`");
    assert_armed_gate_fails(&sink, security, &[], "security", 8);
}

/// Positive control of I7 for the `complexity-*` rules. On the fixed project,
/// `health` reports complexity findings. With the rules at `error`, the
/// enforced `health-findings` gate fails and the machine run exits 1. With the
/// rules at `warn`, the same findings stay in the report, the gate passes and
/// the run exits 0. The generator writes no config file, so without this
/// control no case reaches a `warn` complexity rule.
#[test]
#[ignore = "needs the fallow-mcp binary; run with: cargo build -p fallow-mcp && cargo test -p fallow-cli --test drift -- --include-ignored"]
fn i7_controls_see_complexity_rules_decide_the_health_gate() {
    let health = VERDICT_COMMANDS
        .iter()
        .find(|command| command.args == ["health"])
        .expect("I7 runs `health`");
    for (rule, status, code) in [("error", "fail", 1), ("warn", "pass", 0)] {
        let mut files = fixed_model(false).materialize(true);
        let config = format!(
            r#"{{ "rules": {{ "complexity-cyclomatic": "{rule}", "complexity-cognitive": "{rule}", "complexity-crap": "{rule}" }} }}"#
        );
        files
            .base
            .insert(".fallowrc.json".to_string(), config.clone());
        files.head.insert(".fallowrc.json".to_string(), config);
        let project = Project::from_files(files);
        let runs = verdict_runs(&project, health, &[]);
        project
            .explain(invariants::i7_verdicts_agree(&runs))
            .unwrap_or_else(|err| panic!("{err}"));
        let (_, envelope, exit) = &runs.machine[0];
        let outcome = &envelope["gate_outcomes"]["health-findings"];
        assert!(
            envelope["findings"]
                .as_array()
                .is_some_and(|items| !items.is_empty()),
            "rules {rule}: `health` reports no complexity finding: {envelope}"
        );
        assert!(
            outcome["status"] == status && outcome["enforced"] == true && *exit == code,
            "rules {rule}: `health` must give `health-findings` {status} with exit {code}, \
             got exit {exit}: {}",
            envelope["gate_outcomes"]
        );
    }
}

/// Run `command` with `extra` flags, check I7 on it, and require that the
/// enforced gate `gate` fails and that the machine run exits with `code`.
fn assert_armed_gate_fails(
    project: &Project,
    command: &VerdictCommand,
    extra: &[String],
    gate: &str,
    code: i32,
) {
    let runs = verdict_runs(project, command, extra);
    project
        .explain(invariants::i7_verdicts_agree(&runs))
        .unwrap_or_else(|err| panic!("{err}"));
    let (_, envelope, exit) = &runs.machine[0];
    let outcome = &envelope["gate_outcomes"][gate];
    assert!(
        outcome["status"] == "fail" && outcome["enforced"] == true && *exit == code,
        "{:?} {extra:?} must fail the enforced `{gate}` gate with exit {code}, got exit {exit}: {}",
        command.args,
        envelope["gate_outcomes"]
    );
}

/// Positive control of I5, I7 and I8 for project config. A fixed project has
/// a `.fallowrc.json` with an `overrides` entry for `package.json` and an
/// `ignoreFindings` list. Its head commit adds an unused pnpm dependency
/// override to the manifest and edits two files that export the same name.
///
/// - The `overrides` entry decides the severity of the override finding, so
///   `dead-code --changed-since` and both audit gates must reach the same
///   verdict, and the verdict follows the override.
/// - `ignoreFindings` matches the two edited files. The full run reports the
///   duplicate export, because a third file also exports the name. After
///   `--changed-since`, only ignored files hold it, so every surface hides it.
///
/// The generator writes no config file, so without this control no case
/// reaches per-file severity or `ignoreFindings`.
#[test]
#[ignore = "needs the fallow-mcp binary; run with: cargo build -p fallow-mcp && cargo test -p fallow-cli --test drift -- --include-ignored"]
fn config_controls_see_overrides_and_ignored_duplicates() {
    for (base, manifest, fails) in [("error", "warn", false), ("warn", "error", true)] {
        let project = Project::from_files(config_control_files(base, manifest));
        let context = format!("rules {base}, override for package.json {manifest}");

        let scoped = run_cli(
            &project.root,
            &[
                "dead-code".to_string(),
                "--changed-since".to_string(),
                BASE_REF.to_string(),
            ],
        );
        let scoped = cli_envelope(&scoped);
        assert!(
            scoped["unused_dependency_overrides"]
                .as_array()
                .is_some_and(|items| !items.is_empty()),
            "{context}: the scoped run reports no unused dependency override: {scoped}"
        );
        let dead_code_fails = invariants::stated_verdict(&scoped)
            .expect("dead-code states a verdict")
            .enforced_failure;
        assert_eq!(
            dead_code_fails, fails,
            "{context}: the override decides the dead-code verdict: {}",
            scoped["gate_outcomes"]
        );
        for gate in ["new-only", "all"] {
            let audit = cli_envelope(&run_cli(
                &project.root,
                &[
                    "audit".to_string(),
                    "--base".to_string(),
                    BASE_REF.to_string(),
                    "--gate".to_string(),
                    gate.to_string(),
                ],
            ));
            assert_eq!(
                audit["verdict"] == "fail",
                dead_code_fails,
                "{context}: `audit --gate {gate}` gives verdict {} for the findings that \
                 fail `dead-code --changed-since`: {}",
                audit["verdict"],
                scoped["gate_outcomes"]
            );
        }
        project
            .explain(invariants::i5_audit_surfaces_agree(&[
                ("CLI".to_string(), audit_keys(&cli_audit(&project.root))),
                (
                    "MCP Typed".to_string(),
                    audit_keys(&with_typed_server(|server| {
                        mcp_audit(server, &project.root)
                    })),
                ),
                (
                    "fallow_api".to_string(),
                    audit_keys(&api_audit(&project.root)),
                ),
            ]))
            .unwrap_or_else(|err| panic!("{context}: {err}"));

        let is_duplicate = |key: &FindingKey| key.kind == "duplicate_exports";
        assert!(
            cli_keys(Analysis::DeadCode, &project.root, &Scope::default(), None)
                .iter()
                .any(is_duplicate),
            "{context}: the full run must report the duplicate export"
        );
        let changed = Scope {
            changed_since: Some(BASE_REF.to_string()),
            ..Scope::default()
        };
        let surfaces = all_surfaces(Analysis::DeadCode, &project, &changed);
        project
            .explain(invariants::surfaces_agree(
                "dead-code --changed-since",
                &surfaces,
            ))
            .unwrap_or_else(|err| panic!("{context}: {err}"));
        for (surface, keys) in &surfaces {
            assert!(
                !keys.iter().any(is_duplicate),
                "{context}: {surface} shows a duplicate export that only ignored files hold \
                 after --changed-since\n{}",
                keys::render(keys)
            );
        }

        let regression = regression_args(&project, false);
        for command in VERDICT_COMMANDS {
            let extra = match command.arm {
                Arm::Default => &[][..],
                Arm::Regression => &regression[..],
            };
            project
                .explain(invariants::i7_verdicts_agree(&verdict_runs(
                    &project, command, extra,
                )))
                .unwrap_or_else(|err| panic!("{context}: {err}"));
        }
    }
}

/// The files of the config control. `base` is the severity of
/// `unused-dependency-overrides` in `rules`, and `manifest` is its severity
/// in an `overrides` entry for `package.json`. `duplicate-exports` is `warn`,
/// so only the override finding decides the verdict.
fn config_control_files(base: &str, manifest: &str) -> Materialized {
    let config = format!(
        r#"{{
  "rules": {{ "unused-dependency-overrides": "{base}", "duplicate-exports": "warn" }},
  "overrides": [{{ "files": ["package.json"], "rules": {{ "unused-dependency-overrides": "{manifest}" }} }}],
  "ignoreFindings": ["src/x.ts", "src/y.ts"]
}}
"#
    );
    let package = |overrides: &str| {
        format!(
            r#"{{"name":"drift-config","private":true,"type":"module","main":"src/index.ts"{overrides}}}"#
        )
    };
    let mut base_files = BTreeMap::new();
    base_files.insert("package.json".to_string(), package(""));
    base_files.insert(".fallowrc.json".to_string(), config);
    base_files.insert(
        "src/index.ts".to_string(),
        "export * from \"./x\";\nexport * from \"./y\";\nexport * from \"./z\";\n".to_string(),
    );
    for name in ["x", "y", "z"] {
        base_files.insert(
            format!("src/{name}.ts"),
            "export const dup = 1;\n".to_string(),
        );
    }
    let mut head = base_files.clone();
    head.insert(
        "package.json".to_string(),
        package(r#","pnpm":{"overrides":{"@scope/legacy-pkg":"^1.0.0"}}"#),
    );
    for name in ["x", "y"] {
        head.insert(
            format!("src/{name}.ts"),
            "export const dup = 1;\nconsole.log(dup);\n".to_string(),
        );
    }
    Materialized {
        base: base_files,
        head,
        renames: Vec::new(),
    }
}

/// Members of a saved baseline that identify the file, not its entries.
const BASELINE_HEADER_MEMBERS: &[&str] = &["kind", "analysis_identity"];

/// Keep entry `i` of the saved baseline when `mask[i % len]` is set. Entries
/// are array items and object members below the header.
fn subset_baseline(saved: &Value, mask: &[bool]) -> Value {
    let mut counter = 0usize;
    let mut keep = || {
        let kept = mask.is_empty() || mask[counter % mask.len()];
        counter += 1;
        kept
    };
    let mut out = saved.clone();
    let Some(map) = out.as_object_mut() else {
        return out;
    };
    for (member, value) in map.iter_mut() {
        if BASELINE_HEADER_MEMBERS.contains(&member.as_str()) {
            continue;
        }
        match value {
            Value::Array(items) => items.retain(|_| keep()),
            Value::Object(entries) => entries.retain(|_, _| keep()),
            _ => {}
        }
    }
    out
}

/// A fixed project that yields findings in every domain, so an empty key set
/// from a broken normalizer cannot pass as agreement. The same project also
/// proves that baselines and suppression comments remove findings: without
/// that proof, a run that ignores both would pass every I6 subset check.
#[test]
fn harness_sees_findings_in_every_domain() {
    let project = Project::new(&fixed_model(false), true);
    for analysis in Analysis::ALL {
        let keys = cli_keys(analysis, &project.root, &Scope::default(), None);
        assert!(
            !keys.is_empty(),
            "{analysis:?} reported no findings on the fixed project"
        );
        let api = api_keys(analysis, &project.root, &Scope::default());
        assert_eq!(
            keys, api,
            "{analysis:?} CLI and fallow_api differ on the fixed project"
        );
    }
    for args in [
        vec![],
        vec![
            "audit".to_string(),
            "--base".to_string(),
            BASE_REF.to_string(),
        ],
    ] {
        let envelope = cli_envelope(&run_cli(&project.root, &args));
        assert!(
            !envelope_keys(&envelope).is_empty(),
            "the normalizer for {:?} saw no findings on the fixed project",
            envelope["kind"]
        );
    }
    assert_full_baseline_removes_findings(&project);
    assert_suppressions_remove_findings(&project);
}

/// Baseline half of the I6 positive control.
fn assert_full_baseline_removes_findings(project: &Project) {
    let unscoped = Scope::default();
    for analysis in Analysis::ALL {
        let full = project
            .scratch
            .join(format!("{}-control.json", analysis.cli_command()));
        cli_save_baseline(analysis, &project.root, &full);
        let without = cli_keys(analysis, &project.root, &unscoped, None);
        let with = cli_keys(analysis, &project.root, &unscoped, Some(&full));
        assert!(
            with.is_subset(&without) && with.len() < without.len(),
            "baseline half of I6: a full {analysis:?} baseline removed no finding\n{}",
            invariants::diff("no baseline", &without, "full baseline", &with)
        );
        // Every kind the fixed project yields, dependency kinds included, is
        // stored in a full baseline. A kind that stays means lost coverage.
        assert!(
            with.is_empty(),
            "baseline half of I6: a full {analysis:?} baseline kept findings\n{}",
            keys::render(&with)
        );
    }
}

/// Suppression half of the I6 positive control: the fixed project with
/// suppression comments on the keys of [`suppressed_keys`] reports none of
/// them, on each standalone command and in the combined report. The same
/// project without the comments reports each of them.
fn assert_suppressions_remove_findings(plain: &Project) {
    let suppressed = Project::new(&fixed_model(true), true);
    let unscoped = Scope::default();
    for analysis in Analysis::ALL {
        let without = cli_keys(analysis, &plain.root, &unscoped, None);
        let with = invariants::without_suppression_reports(&cli_keys(
            analysis,
            &suppressed.root,
            &unscoped,
            None,
        ));
        assert_suppressed_keys_removed(&format!("{analysis:?}"), analysis, &without, &with);
    }
    let combined = |project: &Project| combined_keys(&cli_envelope(&run_cli(&project.root, &[])));
    let without = combined(plain);
    let with = combined(&suppressed);
    for (analysis, without, with) in [
        (Analysis::DeadCode, &without.dead_code, &with.dead_code),
        (Analysis::Dupes, &without.dupes, &with.dupes),
        (Analysis::Health, &without.health, &with.health),
    ] {
        let with = invariants::without_suppression_reports(with);
        assert_suppressed_keys_removed(&format!("combined {analysis:?}"), analysis, without, &with);
    }
}

fn assert_suppressed_keys_removed(
    context: &str,
    analysis: Analysis,
    without: &KeySet,
    with: &KeySet,
) {
    assert!(
        with.is_subset(without),
        "suppression half of I6: suppression comments added {context} findings\n{}",
        invariants::diff("plain comments", without, "suppression comments", with)
    );
    // The removed set must be exactly the marked findings. An over-broad
    // comment (a line comment that hides a whole file) removes more keys.
    let marked = suppressed_keys(analysis);
    let unexpected: KeySet = without
        .difference(with)
        .filter(|key| !marked.iter().any(|mark| mark.matches(key)))
        .cloned()
        .collect();
    assert!(
        unexpected.is_empty(),
        "suppression half of I6: suppression comments removed {context} findings that no comment marks\n{}",
        keys::render(&unexpected)
    );
    for marked in marked {
        assert!(
            without.iter().any(|key| marked.matches(key)),
            "suppression half of I6: the {context} run without comments has no {marked:?} finding\n{}",
            keys::render(without)
        );
        let kept: KeySet = with
            .iter()
            .filter(|key| marked.matches(key))
            .cloned()
            .collect();
        assert!(
            kept.is_empty(),
            "suppression half of I6: a suppression comment did not remove the {context} finding {marked:?}\n{}",
            keys::render(&kept)
        );
    }
}

/// A finding that [`fixed_model`] marks with a suppression comment.
#[derive(Debug)]
struct MarkedFinding {
    kind: &'static str,
    /// Matches when the key path contains this file.
    file: &'static str,
    /// Matches any symbol when empty.
    symbol: &'static str,
}

impl MarkedFinding {
    fn matches(&self, key: &FindingKey) -> bool {
        key.kind == self.kind
            && key.path.split(" -> ").any(|path| path == self.file)
            && (self.symbol.is_empty() || key.symbol == self.symbol)
    }
}

/// The findings that `fixed_model(true)` marks with a suppression comment.
/// Keep this list in step with the `suppressed` fields in [`fixed_model`].
fn suppressed_keys(analysis: Analysis) -> &'static [MarkedFinding] {
    const DEAD_CODE: &[MarkedFinding] = &[
        MarkedFinding {
            kind: "unused_exports",
            file: "src/f0.ts",
            symbol: "e0x0",
        },
        MarkedFinding {
            kind: "unused_files",
            file: "src/f3.ts",
            symbol: "",
        },
    ];
    const DUPES: &[MarkedFinding] = &[
        MarkedFinding {
            kind: keys::DUPLICATION_KIND,
            file: "src/f1.ts",
            symbol: "",
        },
        MarkedFinding {
            kind: keys::DUPLICATION_KIND,
            file: "src/f2.ts",
            symbol: "",
        },
    ];
    const HEALTH: &[MarkedFinding] = &[MarkedFinding {
        kind: keys::COMPLEXITY_KIND,
        file: "src/f0.ts",
        symbol: "branchy0",
    }];
    match analysis {
        Analysis::DeadCode => DEAD_CODE,
        Analysis::Dupes => DUPES,
        Analysis::Health => HEALTH,
    }
}

/// The fixed project model. `suppressed` marks one unused export, one unused
/// file, both sites of the clone group and the complex function for a
/// suppression comment (see [`suppressed_keys`]).
fn fixed_model(suppressed: bool) -> ProjectModel {
    use crate::model::{ChangeSpec, DepSpec, ExportSpec, FileSpec, MarkedBlock};
    let file = |exports: usize, imports: Vec<(usize, usize)>| FileSpec {
        second_package: false,
        entry_imported: true,
        suppress_file: false,
        exports: (0..exports)
            .map(|index| ExportSpec {
                is_type: index % 2 == 1,
                suppressed: false,
            })
            .collect(),
        imports,
    };
    let mut first = file(2, vec![(1, 0)]);
    // `e0x0`: an unused value export.
    first.exports[0].suppressed = suppressed;
    // Nothing imports this file, so it is an unused file.
    let orphan = FileSpec {
        entry_imported: false,
        suppress_file: suppressed,
        ..file(0, vec![])
    };
    ProjectModel {
        workspaces: false,
        files: vec![first, file(2, vec![]), file(1, vec![]), orphan],
        deps: vec![DepSpec {
            used_by: None,
            dev: false,
        }],
        duplicate: Some((1, 2, suppressed)),
        complex: Some(MarkedBlock {
            file: 0,
            suppressed,
        }),
        changes: vec![ChangeSpec::Add],
        baseline_mask: vec![true, false],
    }
}
