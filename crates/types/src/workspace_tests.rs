//! Unit tests for the workspace diagnostic kinds and their messages.

use super::*;

#[test]
fn skipped_large_file_diagnostic_id_and_message() {
    let root = Path::new("/project");
    let diag = WorkspaceDiagnostic::new(
        root,
        root.join("src/vendor/app.bundle.js"),
        WorkspaceDiagnosticKind::SkippedLargeFile {
            size_bytes: 6 * 1024 * 1024,
        },
    );
    assert_eq!(diag.kind.id(), "skipped-large-file");
    assert!(
        diag.message.contains("src/vendor/app.bundle.js"),
        "message names the project-relative path: {}",
        diag.message
    );
    assert!(
        diag.message.contains("6.0 MB"),
        "message reports the size: {}",
        diag.message
    );
    assert!(
        diag.message.contains("--max-file-size"),
        "message names the override flag: {}",
        diag.message
    );
}

#[test]
fn skipped_minified_file_diagnostic_id_and_message() {
    let root = Path::new("/project");
    let diag = WorkspaceDiagnostic::new(
        root,
        root.join("src/assets/index-abc123.js"),
        WorkspaceDiagnosticKind::SkippedMinifiedFile {
            size_bytes: 2 * 1024 * 1024,
        },
    );
    assert_eq!(diag.kind.id(), "skipped-minified-file");
    assert!(
        diag.message.contains("src/assets/index-abc123.js"),
        "message names the project-relative path: {}",
        diag.message
    );
    assert!(
        diag.message.contains("2.0 MB"),
        "message reports the size: {}",
        diag.message
    );
    assert!(
        diag.message.contains("--max-file-size 0"),
        "message names the opt-out: {}",
        diag.message
    );
}

#[test]
fn skipped_source_dotdir_diagnostic_id_and_message() {
    let root = Path::new("/project");
    let diag = WorkspaceDiagnostic::new(
        root,
        root.join(".claude"),
        WorkspaceDiagnosticKind::SkippedSourceDotdir,
    );
    assert_eq!(diag.kind.id(), "skipped-source-dotdir");
    assert!(
        diag.message.contains(".claude"),
        "message names the project-relative path: {}",
        diag.message
    );
    assert!(
        diag.message
            .contains("Its imports and exports are not analyzed."),
        "message states the consequence: {}",
        diag.message
    );
    for remedy in [
        "add the file to entry",
        "the export to ignoreExports",
        "the dependency to ignoreDependencies",
    ] {
        assert!(
            diag.message.contains(remedy),
            "message names the remedy `{remedy}`: {}",
            diag.message
        );
    }
    assert!(
        diag.message
            .contains("fallow --root .claude analyzes only that directory")
            && diag.message.contains("does not fix this run"),
        "message must not imply that --root fixes this run: {}",
        diag.message
    );
    assert!(
        diag.message.contains("ignorePatterns"),
        "message names the silencing route: {}",
        diag.message
    );
    assert!(
        diag.message.contains("add '!.claude/**' to ignorePatterns"),
        "the message names the exception that traverses it (issue #2452): {}",
        diag.message
    );
    assert_eq!(
        serde_json::to_value(&diag).expect("serializes")["kind"],
        "skipped-source-dotdir",
        "id() must byte-match the serde kebab-case tag"
    );
}

#[cfg(feature = "schema")]
#[test]
fn workspace_diagnostic_schema_includes_skipped_source_dotdir() {
    let schema = schemars::schema_for!(WorkspaceDiagnostic);
    let json = serde_json::to_string(&schema).expect("schema serializes");
    assert!(json.contains("skipped-source-dotdir"));
}

#[test]
fn source_read_failure_serializes_typed_error_payload() {
    let root = Path::new("/project");
    let diagnostic = WorkspaceDiagnostic::new(
        root,
        root.join("src/removed.ts"),
        WorkspaceDiagnosticKind::SourceReadFailure {
            error: "No such file or directory".to_string(),
        },
    );

    let json = serde_json::to_value(&diagnostic).expect("diagnostic serializes");
    assert_eq!(json["kind"], "source-read-failure");
    assert_eq!(
        json["path"],
        root.join("src/removed.ts")
            .display()
            .to_string()
            .replace('\\', "/")
    );
    assert_eq!(json["error"], "No such file or directory");
    assert!(
        json["message"]
            .as_str()
            .is_some_and(|message| message.contains("src/removed.ts"))
    );
}

#[cfg(feature = "schema")]
#[test]
fn workspace_diagnostic_schema_includes_source_read_failure() {
    let schema = schemars::schema_for!(WorkspaceDiagnostic);
    let json = serde_json::to_string(&schema).expect("schema serializes");
    assert!(json.contains("source-read-failure"));
    assert!(json.contains("error"));
}

#[test]
fn bun_lockb_override_resolution_skipped_id_and_message() {
    let root = Path::new("/project");
    let diag = WorkspaceDiagnostic::new(
        root,
        root.join("package.json"),
        WorkspaceDiagnosticKind::BunLockbOverrideResolutionSkipped,
    );
    assert_eq!(diag.kind.id(), "bun-lockb-override-resolution-skipped");
    assert!(
        diag.message.contains("'package.json'"),
        "message names the project-relative manifest: {}",
        diag.message
    );
    assert!(
        diag.message.contains("no parseable text lockfile"),
        "message states the cause: {}",
        diag.message
    );
    assert!(
        !diag.message.contains("only bun.lockb"),
        "message must not claim bun.lockb is the only lockfile; yarn.lock or an unparseable \
         bun.lock may sit beside it: {}",
        diag.message
    );
    assert!(
        diag.message.contains("bun install --save-text-lockfile")
            && diag.message.contains("delete the stale bun.lockb"),
        "message ends with the text-lockfile next step and the stale-lockb alternative: {}",
        diag.message
    );
    let json = serde_json::to_value(&diag).expect("diagnostic serializes");
    assert_eq!(json["kind"], "bun-lockb-override-resolution-skipped");
}

#[test]
fn lockfile_override_diagnostic_ids_and_messages_are_actionable() {
    let root = Path::new("/project");
    let malformed = WorkspaceDiagnostic::new(
        root,
        root.join("bun.lock"),
        WorkspaceDiagnosticKind::BunLockOverrideResolutionSkipped,
    );
    assert_eq!(malformed.kind.id(), "bun-lock-override-resolution-skipped");
    assert!(malformed.message.contains("regenerate"));

    let pnpm = WorkspaceDiagnostic::new(
        root,
        root.join("pnpm-lock.yaml"),
        WorkspaceDiagnosticKind::PnpmLockOverrideResolutionSkipped,
    );
    assert_eq!(pnpm.kind.id(), "pnpm-lock-override-resolution-skipped");
    assert!(pnpm.message.contains("merge-conflict markers"));
    assert!(pnpm.message.contains("pnpm install"));

    let npm = WorkspaceDiagnostic::new(
        root,
        root.join("package-lock.json"),
        WorkspaceDiagnosticKind::NpmLockOverrideResolutionSkipped,
    );
    assert_eq!(npm.kind.id(), "npm-lock-override-resolution-skipped");
    assert!(npm.message.contains("package-lock.json"));
    assert!(npm.message.contains("merge-conflict markers"));
    assert!(npm.message.contains("npm install"));

    let shadowed = WorkspaceDiagnostic::new(
        root,
        root.join("package.json"),
        WorkspaceDiagnosticKind::BunResolutionsShadowedByOverrides,
    );
    assert_eq!(shadowed.kind.id(), "bun-resolutions-shadowed-by-overrides");
    assert!(shadowed.message.contains("ignores `resolutions`"));

    let ignored = WorkspaceDiagnostic::new(
        root,
        root.join("pnpm-workspace.yaml"),
        WorkspaceDiagnosticKind::PnpmWorkspaceOverridesIgnored {
            cause: PnpmWorkspaceOverridesIgnoredCause::PackageJsonOverrides,
        },
    );
    assert_eq!(ignored.kind.id(), "pnpm-workspace-overrides-ignored");
    assert!(ignored.message.contains("pnpm-workspace.yaml"));
    assert!(
        ignored
            .message
            .contains("pnpm 10 and earlier ignore the `overrides`")
    );

    let too_old = WorkspaceDiagnostic::new(
        root,
        root.join("pnpm-workspace.yaml"),
        WorkspaceDiagnosticKind::PnpmWorkspaceOverridesIgnored {
            cause: PnpmWorkspaceOverridesIgnoredCause::PnpmVersion,
        },
    );
    assert_eq!(too_old.kind.id(), "pnpm-workspace-overrides-ignored");
    assert!(too_old.message.contains("pnpm-workspace.yaml"));
    assert!(
        too_old
            .message
            .contains("pnpm versions before 10.5.1 do not read the `overrides`")
    );
    let json = serde_json::to_value(&too_old).expect("diagnostic serializes");
    assert_eq!(json["kind"], "pnpm-workspace-overrides-ignored");
    assert_eq!(json["cause"], "pnpm-version");
    let json = serde_json::to_value(&ignored).expect("diagnostic serializes");
    assert_eq!(json["cause"], "package-json-overrides");
}

#[test]
fn into_root_relative_strips_the_root_and_keeps_outside_paths_absolute() {
    let root = Path::new("/project");
    let inside = WorkspaceDiagnostic::new(
        root,
        root.join("packages/inner"),
        WorkspaceDiagnosticKind::UndeclaredWorkspace,
    )
    .into_root_relative(root);
    assert_eq!(inside.path, Path::new("packages/inner"));

    let outside = WorkspaceDiagnostic::new(
        root,
        PathBuf::from("/elsewhere/packages/inner"),
        WorkspaceDiagnosticKind::UndeclaredWorkspace,
    )
    .into_root_relative(root);
    assert_eq!(outside.path, Path::new("/elsewhere/packages/inner"));
}

#[test]
fn analysis_stage_classification_covers_only_analyze_stage_kinds() {
    let analysis_stage = [
        WorkspaceDiagnosticKind::MalformedPnpmWorkspaceYaml {
            error: "bad yaml".to_owned(),
        },
        WorkspaceDiagnosticKind::BunLockbOverrideResolutionSkipped,
        WorkspaceDiagnosticKind::BunLockOverrideResolutionSkipped,
        WorkspaceDiagnosticKind::PnpmLockOverrideResolutionSkipped,
        WorkspaceDiagnosticKind::NpmLockOverrideResolutionSkipped,
        WorkspaceDiagnosticKind::BunResolutionsShadowedByOverrides,
        WorkspaceDiagnosticKind::PnpmWorkspaceOverridesIgnored {
            cause: PnpmWorkspaceOverridesIgnoredCause::PnpmVersion,
        },
    ];
    for kind in &analysis_stage {
        assert!(
            kind.is_analysis_stage() && !kind.is_source_discovery(),
            "{} is recorded by the analyze stage only",
            kind.id()
        );
    }

    let other = [
        WorkspaceDiagnosticKind::UndeclaredWorkspace,
        WorkspaceDiagnosticKind::MalformedPackageJson {
            error: "trailing comma".to_owned(),
        },
        WorkspaceDiagnosticKind::GlobMatchedNoPackageJson {
            pattern: "packages/*".to_owned(),
        },
        WorkspaceDiagnosticKind::MalformedTsconfig {
            error: "unexpected token".to_owned(),
        },
        WorkspaceDiagnosticKind::TsconfigReferenceDirMissing,
        WorkspaceDiagnosticKind::SkippedLargeFile { size_bytes: 1 },
        WorkspaceDiagnosticKind::SkippedMinifiedFile { size_bytes: 1 },
        WorkspaceDiagnosticKind::SkippedSourceDotdir,
        WorkspaceDiagnosticKind::SourceReadFailure {
            error: "permission denied".to_owned(),
        },
    ];
    for kind in &other {
        assert!(
            !kind.is_analysis_stage(),
            "{} is a discovery kind, not an analyze-stage kind",
            kind.id()
        );
    }
}

#[test]
fn merge_keeps_two_diagnostics_that_share_a_kind_id_and_path() {
    let root = Path::new("/project");
    let first = WorkspaceDiagnostic::new(
        root,
        root.join("packages/aaa"),
        WorkspaceDiagnosticKind::GlobMatchedNoPackageJson {
            pattern: "packages/*".to_owned(),
        },
    );
    let second = WorkspaceDiagnostic::new(
        root,
        root.join("packages/aaa"),
        WorkspaceDiagnosticKind::GlobMatchedNoPackageJson {
            pattern: "packages/a*".to_owned(),
        },
    );

    let merged =
        merge_workspace_diagnostics(vec![first.clone(), second.clone()], vec![first, second]);

    let patterns: Vec<String> = merged
        .iter()
        .map(|diagnostic| match &diagnostic.kind {
            WorkspaceDiagnosticKind::GlobMatchedNoPackageJson { pattern } => pattern.clone(),
            other => panic!("unexpected kind {}", other.id()),
        })
        .collect();
    assert_eq!(
        patterns,
        ["packages/*", "packages/a*"],
        "two overlapping globs report the same directory twice, with their own pattern; \
         the same entry seen from two observation points still folds to one"
    );
}

/// Issue #2366: a repository that declares one glob in two manifests
/// (`"./apps/**"` in `package.json`, `apps/**` in `pnpm-workspace.yaml`)
/// must not report every package-less directory under it twice now that the
/// payload is part of the dedupe key.
#[test]
fn merge_folds_two_spellings_of_one_glob_into_one_diagnostic() {
    let root = Path::new("/project");
    let dotted = WorkspaceDiagnostic::new(
        root,
        root.join("apps/site/.next/cache"),
        WorkspaceDiagnosticKind::GlobMatchedNoPackageJson {
            pattern: "./apps/**".to_owned(),
        },
    );
    let bare = WorkspaceDiagnostic::new(
        root,
        root.join("apps/site/.next/cache"),
        WorkspaceDiagnosticKind::GlobMatchedNoPackageJson {
            pattern: "apps/**".to_owned(),
        },
    );
    assert_eq!(
        dotted.kind, bare.kind,
        "the no-op ./ prefix is normalised out of the recorded pattern"
    );
    assert!(
        dotted.message.contains("Glob 'apps/**'"),
        "the message renders the normalised pattern: {}",
        dotted.message
    );

    let merged = merge_workspace_diagnostics(vec![dotted], vec![bare]);
    assert_eq!(
        merged.len(),
        1,
        "one glob declared twice is one diagnostic: {merged:?}"
    );
}

/// A glob spelled exactly `"./"` (the project root itself) is the one
/// pattern the prefix strip must leave alone: an empty `pattern` field
/// names no glob, and the warning would quote nothing.
#[test]
fn new_keeps_a_root_only_glob_spelling_and_still_strips_a_real_prefix() {
    let root = Path::new("/project");
    let recorded = |pattern: &str| {
        let diagnostic = WorkspaceDiagnostic::new(
            root,
            root.join("pkgs"),
            WorkspaceDiagnosticKind::GlobMatchedNoPackageJson {
                pattern: pattern.to_owned(),
            },
        );
        let WorkspaceDiagnosticKind::GlobMatchedNoPackageJson { pattern } = diagnostic.kind else {
            panic!("constructed a glob-matched-no-package-json diagnostic");
        };
        (pattern, diagnostic.message)
    };

    let (root_pattern, root_message) = recorded("./");
    assert_eq!(root_pattern, "./", "a root-only glob keeps its spelling");
    assert!(
        root_message.contains("Glob './'"),
        "the warning names the glob the manifest declared: {root_message}"
    );
    assert_eq!(recorded(".\\").0, ".\\");
    assert_eq!(recorded("./pkgs/*").0, "pkgs/*");
    assert_eq!(recorded(".\\pkgs\\*").0, "pkgs\\*");
}

/// Issue #2366, the path half of the same repository shape: expanding
/// `./pkgs/*` joins the no-op `.` into every match, so the two manifests
/// hand one directory to the diagnostic under two spellings. Both must
/// store, render and serialise as the bare one, otherwise whichever
/// manifest was read first decides whether the analysis envelopes print
/// `./pkgs/aaa` while the workspace listing envelope prints `pkgs/aaa`.
#[test]
fn new_stores_one_spelling_for_a_directory_reached_through_a_dotted_glob() {
    let root = Path::new("/project");
    let dotted = WorkspaceDiagnostic::new(
        root,
        root.join("./pkgs/aaa"),
        WorkspaceDiagnosticKind::GlobMatchedNoPackageJson {
            pattern: "./pkgs/*".to_owned(),
        },
    );
    let bare = WorkspaceDiagnostic::new(
        root,
        root.join("pkgs/aaa"),
        WorkspaceDiagnosticKind::GlobMatchedNoPackageJson {
            pattern: "pkgs/*".to_owned(),
        },
    );

    let spelling =
        |diagnostic: &WorkspaceDiagnostic| diagnostic.path.display().to_string().replace('\\', "/");
    assert_eq!(
        spelling(&dotted),
        "/project/pkgs/aaa",
        "the stored path drops the no-op . component, which Path equality \
         hides but serialization does not"
    );
    assert_eq!(spelling(&dotted), spelling(&bare));
    assert_eq!(
        spelling(&dotted.clone().into_root_relative(root)),
        "pkgs/aaa"
    );

    let merged = merge_workspace_diagnostics(vec![dotted], vec![bare]);
    assert_eq!(
        merged.len(),
        1,
        "one directory reached through two spellings of one glob: {merged:?}"
    );
}

/// The single-list fold applied at workspace discovery keeps one entry per
/// `(kind, path)` and leaves distinct payloads alone.
#[test]
fn dedupe_keeps_first_of_each_pair_and_every_distinct_payload() {
    let root = Path::new("/project");
    let glob = |pattern: &str, relative: &str| {
        WorkspaceDiagnostic::new(
            root,
            root.join(relative),
            WorkspaceDiagnosticKind::GlobMatchedNoPackageJson {
                pattern: pattern.to_owned(),
            },
        )
    };

    let deduped = dedupe_workspace_diagnostics(vec![
        glob("pkgs/*", "pkgs/aaa"),
        glob("pkgs/*", "pkgs/bbb"),
        glob("./pkgs/*", "./pkgs/aaa"),
        glob("pkgs/a*", "pkgs/aaa"),
    ]);

    let reported: Vec<(String, String)> = deduped
        .iter()
        .map(|diagnostic| match &diagnostic.kind {
            WorkspaceDiagnosticKind::GlobMatchedNoPackageJson { pattern } => (
                pattern.clone(),
                diagnostic.path.display().to_string().replace('\\', "/"),
            ),
            other => panic!("unexpected kind {}", other.id()),
        })
        .collect();

    assert_eq!(
        reported,
        vec![
            ("pkgs/*".to_owned(), "/project/pkgs/aaa".to_owned()),
            ("pkgs/*".to_owned(), "/project/pkgs/bbb".to_owned()),
            ("pkgs/a*".to_owned(), "/project/pkgs/aaa".to_owned()),
        ],
        "the duplicate spelling folds away and the overlapping glob stays"
    );
}

/// The class `reachability_caveats[]` is computed from. Every kind here
/// means the run never read a file that is part of the project, so its
/// imports credit nothing and the modules it imports can be reported
/// unused with a removal action on them. Classifying a kind `true` is the
/// only wiring its findings need to inherit the caveat and the `fallow fix`
/// withholding that follows it.
#[test]
fn source_never_analyzed_covers_every_file_the_run_did_not_read() {
    for kind in [
        WorkspaceDiagnosticKind::SkippedLargeFile { size_bytes: 1 },
        WorkspaceDiagnosticKind::SkippedMinifiedFile { size_bytes: 1 },
        WorkspaceDiagnosticKind::SkippedSourceDotdir,
        WorkspaceDiagnosticKind::SourceReadFailure {
            error: "permission denied".to_owned(),
        },
    ] {
        assert!(
            kind.source_never_analyzed(),
            "{} names a source file this run never read",
            kind.id()
        );
    }

    let degraded = WorkspaceDiagnosticKind::SourceParseDegraded {
        error_count: 3,
        panicked: false,
    };
    assert!(
        !degraded.source_never_analyzed(),
        "a degraded parse read the file, so it has a graph node and its reachability is \
         observable; the caveat pass narrows it instead of treating it as unread"
    );

    for kind in [
        WorkspaceDiagnosticKind::UndeclaredWorkspace,
        WorkspaceDiagnosticKind::MalformedPackageJson {
            error: "trailing comma".to_owned(),
        },
        WorkspaceDiagnosticKind::GlobMatchedNoPackageJson {
            pattern: "packages/*".to_owned(),
        },
        WorkspaceDiagnosticKind::MalformedTsconfig {
            error: "unexpected token".to_owned(),
        },
        WorkspaceDiagnosticKind::TsconfigReferenceDirMissing,
        WorkspaceDiagnosticKind::MalformedPnpmWorkspaceYaml {
            error: "bad indent".to_owned(),
        },
        WorkspaceDiagnosticKind::BunLockbOverrideResolutionSkipped,
        WorkspaceDiagnosticKind::BunLockOverrideResolutionSkipped,
        WorkspaceDiagnosticKind::PnpmLockOverrideResolutionSkipped,
        WorkspaceDiagnosticKind::NpmLockOverrideResolutionSkipped,
        WorkspaceDiagnosticKind::BunResolutionsShadowedByOverrides,
        WorkspaceDiagnosticKind::PnpmWorkspaceOverridesIgnored {
            cause: PnpmWorkspaceOverridesIgnoredCause::PnpmVersion,
        },
        WorkspaceDiagnosticKind::NodeModulesMissing,
        WorkspaceDiagnosticKind::BoundariesNotConfigured,
        WorkspaceDiagnosticKind::RulePacksNotConfigured,
    ] {
        assert!(
            !kind.source_never_analyzed(),
            "{} says nothing about a source file's imports going unseen",
            kind.id()
        );
    }
}

#[test]
fn source_walk_recorded_covers_only_the_kinds_a_walk_replaces() {
    for kind in [
        WorkspaceDiagnosticKind::SkippedLargeFile { size_bytes: 1 },
        WorkspaceDiagnosticKind::SkippedMinifiedFile { size_bytes: 1 },
        WorkspaceDiagnosticKind::SkippedSourceDotdir,
    ] {
        assert!(
            kind.is_source_walk_recorded() && kind.is_source_discovery(),
            "{} is written by the source walk",
            kind.id()
        );
    }

    let read_failure = WorkspaceDiagnosticKind::SourceReadFailure {
        error: "permission denied".to_owned(),
    };
    assert!(
        read_failure.is_source_discovery() && !read_failure.is_source_walk_recorded(),
        "the parse stage records source-read-failure after the walk, so it must keep \
         reaching sessions through the registry"
    );

    for kind in [
        WorkspaceDiagnosticKind::UndeclaredWorkspace,
        WorkspaceDiagnosticKind::TsconfigReferenceDirMissing,
        WorkspaceDiagnosticKind::BunLockbOverrideResolutionSkipped,
        WorkspaceDiagnosticKind::BunLockOverrideResolutionSkipped,
        WorkspaceDiagnosticKind::PnpmLockOverrideResolutionSkipped,
        WorkspaceDiagnosticKind::NpmLockOverrideResolutionSkipped,
        WorkspaceDiagnosticKind::BunResolutionsShadowedByOverrides,
        WorkspaceDiagnosticKind::PnpmWorkspaceOverridesIgnored {
            cause: PnpmWorkspaceOverridesIgnoredCause::PnpmVersion,
        },
    ] {
        assert!(
            !kind.is_source_walk_recorded(),
            "{} is not written by the source walk",
            kind.id()
        );
    }
}

/// Issue #2638, the single most load-bearing classification in the new
/// kind. Answering `true` here would attach `IncompleteFileAnalysis` and
/// `IncompleteImportGraph` caveats to findings on nearly every project
/// that keeps a non-gitignored `dist/` or `coverage/`, and make
/// `fallow fix` withhold `delete-file` and `remove-export` project-wide.
/// A built-in exclusion is designed behavior on generated output, not a
/// degraded run.
#[test]
fn a_built_in_ignore_exclusion_is_not_a_file_the_run_failed_to_analyze() {
    let kind = WorkspaceDiagnosticKind::ExcludedByDefaultIgnore {
        pattern: "**/build/**".to_owned(),
        file_count: 3,
        directory_count: 1,
    };
    assert!(!kind.source_never_analyzed());
}

/// Issue #2638: these exclusions fire in the product's default state on
/// most monorepos, so a default stderr line would be permanent noise that
/// names no defect. The CLI prints a note under `--explain-skipped`
/// instead.
#[test]
fn a_built_in_ignore_exclusion_does_not_warn_on_stderr_by_default() {
    let kind = WorkspaceDiagnosticKind::ExcludedByDefaultIgnore {
        pattern: "**/build/**".to_owned(),
        file_count: 3,
        directory_count: 1,
    };
    assert!(!kind.warns_on_stderr());
}

/// Issue #2638 plus issue #2366: the walk writes it, so it has to be
/// classified as source-discovery (or combined mode's per-analysis config
/// reloads wipe it before serialization) AND as walk-recorded (or a
/// concurrent walk's tally is folded into another analysis's list).
#[test]
fn a_built_in_ignore_exclusion_is_walk_recorded_source_discovery() {
    let kind = WorkspaceDiagnosticKind::ExcludedByDefaultIgnore {
        pattern: "**/build/**".to_owned(),
        file_count: 3,
        directory_count: 1,
    };
    assert!(kind.is_source_discovery());
    assert!(kind.is_source_walk_recorded());
    assert!(!kind.is_analysis_stage());
    assert_eq!(kind.id(), "excluded-by-default-ignore");
}

/// Issue #2638: the message has to name the pattern the reader cannot see,
/// the directory, and the remedies that analyze the tree: a `!` exception
/// in `ignorePatterns` (issue #2940) and `--root`.
#[test]
fn a_built_in_ignore_exclusion_message_names_the_pattern_and_the_root_remedy() {
    let root = Path::new("/project");
    let diag = WorkspaceDiagnostic::new(
        root,
        root.join("packages/web/build"),
        WorkspaceDiagnosticKind::ExcludedByDefaultIgnore {
            pattern: "**/build/**".to_owned(),
            file_count: 4,
            directory_count: 1,
        },
    );
    assert!(diag.message.contains("**/build/**"), "{}", diag.message);
    assert!(
        diag.message.contains("packages/web/build"),
        "{}",
        diag.message
    );
    assert!(
        diag.message.contains("fallow --root packages/web/build"),
        "the remedy is copy-pasteable: {}",
        diag.message
    );
    assert!(
        diag.message
            .contains("add '!packages/web/build/**' to ignorePatterns"),
        "the message names the exception that lifts the built-in (issue #2940): {}",
        diag.message
    );
}

/// One excluded file reads as one file, not as "1 source files".
#[test]
fn a_single_excluded_file_message_is_singular() {
    let root = Path::new("/project");
    let diag = WorkspaceDiagnostic::new(
        root,
        root.join("dist"),
        WorkspaceDiagnosticKind::ExcludedByDefaultIgnore {
            pattern: "**/dist/**".to_owned(),
            file_count: 1,
            directory_count: 1,
        },
    );
    assert!(
        diag.message
            .starts_with("Skipped 1 source file under 'dist'"),
        "{}",
        diag.message
    );
    assert!(diag.message.contains("it matches"), "{}", diag.message);
    assert!(
        diag.message
            .contains("nothing it imports, exports, or defines"),
        "the whole sentence agrees in number, not just its first clause: {}",
        diag.message
    );
}

/// The anchor directory is the largest group, never a majority: ten
/// packages each holding one excluded file make every one of them "the
/// largest", and a message claiming otherwise is false on exactly the flat
/// monorepo shape issue #2638 is about.
#[test]
fn a_scattered_exclusion_names_the_largest_group_and_counts_the_directories() {
    let root = Path::new("/project");
    let diag = WorkspaceDiagnostic::new(
        root,
        root.join("packages/a/dist"),
        WorkspaceDiagnosticKind::ExcludedByDefaultIgnore {
            pattern: "**/dist/**".to_owned(),
            file_count: 10,
            directory_count: 10,
        },
    );
    assert!(
        diag.message.starts_with(
            "Skipped 10 source files across 10 directories, the largest group under \
             'packages/a/dist'"
        ),
        "{}",
        diag.message
    );
    assert!(
        !diag.message.contains("the most of them"),
        "a max-of-group is not a majority: {}",
        diag.message
    );
}

/// A file-shaped built-in matches on the file name, so the `--root` remedy
/// the directory-shaped patterns get would re-exclude the same file. The
/// message must not print a command that provably does nothing.
#[test]
fn a_file_shaped_pattern_does_not_advertise_the_root_remedy() {
    let root = Path::new("/project");
    let diag = WorkspaceDiagnostic::new(
        root,
        root.join("vendor"),
        WorkspaceDiagnosticKind::ExcludedByDefaultIgnore {
            pattern: "**/*.min.js".to_owned(),
            file_count: 2,
            directory_count: 1,
        },
    );
    assert!(
        !diag.message.contains("fallow --root"),
        "the message explains why re-rooting fails, it does not prescribe it: {}",
        diag.message
    );
    assert!(
        diag.message.contains("matches a file name"),
        "the message says why: {}",
        diag.message
    );
    assert!(
        diag.message.contains("Rename"),
        "and names the remedy that does work: {}",
        diag.message
    );
}

/// A built-in that matched a file sitting directly at the analysis root
/// anchors at the root, and an empty string is not a location.
#[test]
fn a_root_anchored_exclusion_renders_its_location_as_dot() {
    let root = Path::new("/project");
    let diag = WorkspaceDiagnostic::new(
        root,
        root.to_path_buf(),
        WorkspaceDiagnosticKind::ExcludedByDefaultIgnore {
            pattern: "**/*.min.js".to_owned(),
            file_count: 1,
            directory_count: 1,
        },
    );
    assert!(
        diag.message.starts_with("Skipped 1 source file under '.'"),
        "{}",
        diag.message
    );
}

#[test]
fn glob_first_literal_segment_skips_wildcards_and_dot_components() {
    assert_eq!(glob_first_literal_segment("**/build/**"), Some("build"));
    assert_eq!(glob_first_literal_segment("./dist/**"), Some("dist"));
    assert_eq!(glob_first_literal_segment("**/*.min.js"), None);
    assert_eq!(glob_first_literal_segment("**/*.bundle.js"), None);
    assert_eq!(glob_first_literal_segment("**/{a,b}/**"), None);
}

#[test]
fn format_size_mb_one_decimal() {
    assert_eq!(format_size_mb(0), "0.0 MB");
    assert_eq!(format_size_mb(5 * 1024 * 1024), "5.0 MB");
    assert_eq!(format_size_mb(1024 * 1024 + 512 * 1024), "1.5 MB");
}

#[test]
fn unmatched_config_pattern_kinds_are_advisory_root_notes() {
    let root = Path::new("/project");
    for (kind, id, setting, pattern) in [
        (
            WorkspaceDiagnosticKind::IgnoreDependenciesGlobUnmatched {
                pattern: "@typo/*".to_owned(),
            },
            "ignore-dependencies-glob-unmatched",
            "ignoreDependencies",
            "@typo/*",
        ),
        (
            WorkspaceDiagnosticKind::IgnoreFindingsPatternUnmatched {
                pattern: "src/legcy/**".to_owned(),
            },
            "ignore-findings-pattern-unmatched",
            "ignoreFindings",
            "src/legcy/**",
        ),
    ] {
        let diag = WorkspaceDiagnostic::new(root, root.to_path_buf(), kind);
        assert_eq!(diag.kind.id(), id);
        assert_eq!(
            diag.kind.unmatched_config_pattern(),
            Some((setting, pattern))
        );
        assert!(!diag.degrades_analysis, "{id} does not degrade the run");
        assert!(!diag.kind.source_never_analyzed());
        assert!(
            !diag.kind.is_analysis_stage(),
            "{id} is not in the registry"
        );
        assert!(!diag.kind.is_health_stage());
        assert!(!diag.kind.is_plugin_stage());
        assert!(!diag.kind.is_source_discovery());
        assert!(diag.message.contains(setting), "{}", diag.message);
        assert!(diag.message.contains(pattern), "{}", diag.message);
        assert!(
            diag.message.contains("remove it from the config"),
            "next step: {}",
            diag.message
        );
        let json = serde_json::to_value(&diag).expect("serializes");
        assert_eq!(json["kind"], id);
        assert_eq!(json["pattern"], pattern);
        assert!(json.get("degrades_analysis").is_none());
    }
    assert!(
        WorkspaceDiagnosticKind::UndeclaredWorkspace
            .unmatched_config_pattern()
            .is_none()
    );
}

#[test]
fn undeclared_workspace_message_has_next_step() {
    let root = Path::new("/project");
    let diag = WorkspaceDiagnostic::new(
        root,
        root.join("packages/legacy"),
        WorkspaceDiagnosticKind::UndeclaredWorkspace,
    );
    assert_eq!(diag.kind.id(), "undeclared-workspace");
    assert!(diag.message.contains("packages/legacy"), "{}", diag.message);
    assert!(
        diag.message.contains("ignorePatterns"),
        "next-step hint preserved: {}",
        diag.message
    );
}
/// The seven health-pipeline kinds (issue #2689). Each is classified in
/// four places, and getting one wrong is silent: an entry that answers
/// `is_analysis_stage` is wiped by the dead-code pass the health run itself
/// invokes, and one that answers `is_source_discovery` is preserved by the
/// wrong mechanism.
#[test]
fn health_stage_kinds_are_classified_as_health_stage_and_nothing_else() {
    for kind in [
        WorkspaceDiagnosticKind::FileScoresUnavailable {
            error: "boom".to_owned(),
        },
        WorkspaceDiagnosticKind::HotspotsSkipped {
            cause: "not-a-repository".to_owned(),
        },
        WorkspaceDiagnosticKind::ShallowClone {
            ownership_requested: true,
        },
        WorkspaceDiagnosticKind::UnpinnedClock,
        WorkspaceDiagnosticKind::OwnershipUnavailable {
            cause: "codeowners-parse-failed".to_owned(),
            error: "boom".to_owned(),
        },
        WorkspaceDiagnosticKind::TrendSnapshotUnreadable {
            error: "boom".to_owned(),
        },
        WorkspaceDiagnosticKind::CoverageAutoDetected,
    ] {
        let id = kind.id();
        assert!(kind.is_health_stage(), "{id} must be health-stage");
        assert!(!kind.is_analysis_stage(), "{id} must not be analysis-stage");
        assert!(
            !kind.is_source_discovery(),
            "{id} must not be source-discovery"
        );
        assert!(
            !kind.is_source_walk_recorded(),
            "{id} must not be walk-recorded"
        );
        assert!(
            !kind.source_never_analyzed(),
            "{id} reports an input, not an unread source file"
        );
    }
}

/// Six of the seven report a result the run could not measure as asked;
/// the coverage provenance entry does not, and a consumer sentence about a
/// degraded run must not fire for it.
#[test]
fn only_the_coverage_provenance_kind_does_not_degrade_the_analysis() {
    assert!(
        WorkspaceDiagnosticKind::HotspotsSkipped {
            cause: "invalid-since".to_owned(),
        }
        .warns_on_stderr(),
        "a skipped hotspot section is a degraded result"
    );
    assert!(
        WorkspaceDiagnosticKind::UnpinnedClock.warns_on_stderr(),
        "a drifting measurement is a degraded result"
    );
    assert!(
        !WorkspaceDiagnosticKind::CoverageAutoDetected.warns_on_stderr(),
        "auto-detected coverage loaded fine and degraded nothing"
    );
}

/// Each skip cause carries its own remedy, and the original cause's wording
/// is frozen: it shipped before the token existed, so a reader who matched
/// on that sentence must still match on it.
#[test]
fn every_hotspot_skip_cause_renders_its_own_remedy() {
    let root = Path::new("/project");
    let skipped = |cause: &str, path: PathBuf| {
        WorkspaceDiagnostic::new(
            root,
            path,
            WorkspaceDiagnosticKind::HotspotsSkipped {
                cause: cause.to_owned(),
            },
        )
    };

    let no_repo = skipped("not-a-repository", root.to_path_buf());
    assert_eq!(
        no_repo.message,
        "Hotspot analysis was skipped because no git repository was found at the project \
         root, so the hotspots, churn and ownership sections report nothing rather than \
         zero. Run fallow inside the repository, or pass --churn-file with exported change \
         history."
    );

    let bad_since = skipped("invalid-since", root.to_path_buf());
    assert!(
        bad_since.message.contains("--since")
            && bad_since.message.contains("6m or 90d")
            && !bad_since.message.contains("no git repository"),
        "a malformed window is respelled, not moved into a repository: {}",
        bad_since.message
    );

    let churn = skipped("churn-file-unreadable", root.join("build/churn.json"));
    assert!(
        churn.message.contains("'build/churn.json'") && churn.message.contains("rerun"),
        "the remedy names the file that changed under the run: {}",
        churn.message
    );

    let unborn = skipped("no-commits", root.to_path_buf());
    assert!(
        unborn.message.contains("no commits")
            && unborn.message.contains("--churn-file")
            && !unborn.message.contains("no git repository"),
        "a branch without a commit is told to commit, not to move: {}",
        unborn.message
    );

    for diagnostic in [&no_repo, &unborn, &bad_since, &churn] {
        assert!(
            diagnostic.degrades_analysis,
            "every skip leaves the hotspot sections unmeasured: {}",
            diagnostic.message
        );
    }
}

/// The message is the only prose a consumer renders, so each one must name
/// the consequence and a next step rather than restate the kind.
#[test]
fn health_stage_messages_name_a_next_step() {
    let root = Path::new("/project");
    let shallow = WorkspaceDiagnostic::new(
        root,
        root.to_path_buf(),
        WorkspaceDiagnosticKind::ShallowClone {
            ownership_requested: true,
        },
    );
    assert!(
        shallow.message.contains("git fetch --unshallow"),
        "{}",
        shallow.message
    );
    assert!(
        shallow.message.contains("Ownership signals are skewed too"),
        "a run that asked for ownership is told what else it costs: {}",
        shallow.message
    );
    let without_ownership = WorkspaceDiagnostic::new(
        root,
        root.to_path_buf(),
        WorkspaceDiagnosticKind::ShallowClone {
            ownership_requested: false,
        },
    );
    assert!(
        !without_ownership.message.contains("Ownership"),
        "a run that did not ask for ownership is not told about it: {}",
        without_ownership.message
    );

    let coverage = WorkspaceDiagnostic::new(
        root,
        root.join("coverage/coverage-final.json"),
        WorkspaceDiagnosticKind::CoverageAutoDetected,
    );
    assert_eq!(coverage.kind.id(), "coverage-auto-detected");
    assert!(
        coverage
            .message
            .contains("--coverage 'coverage/coverage-final.json'"),
        "the remedy names the file that fed the score: {}",
        coverage.message
    );
    assert!(
        !coverage.degrades_analysis,
        "provenance is not a degraded run"
    );

    let ownership = WorkspaceDiagnostic::new(
        root,
        root.to_path_buf(),
        WorkspaceDiagnosticKind::OwnershipUnavailable {
            cause: "invalid-bot-pattern".to_owned(),
            error: "unclosed".to_owned(),
        },
    );
    assert!(
        ownership.message.contains("botPatterns"),
        "the two causes render different remedies: {}",
        ownership.message
    );
}

fn plugin_unreadable(key: &str, reason: &str) -> WorkspaceDiagnostic {
    WorkspaceDiagnostic::new(
        Path::new("/project"),
        PathBuf::from("/project/module-federation.config.ts"),
        WorkspaceDiagnosticKind::PluginConfigUnreadable {
            plugin: "module-federation".to_owned(),
            key: key.to_owned(),
            reason: reason.to_owned(),
        },
    )
}

fn plugin_not_modeled(key: &str, reason: &str) -> WorkspaceDiagnostic {
    WorkspaceDiagnostic::new(
        Path::new("/project"),
        PathBuf::from("/project/nuxt.config.ts"),
        WorkspaceDiagnosticKind::PluginEffectNotModeled {
            plugin: "nuxt".to_owned(),
            key: key.to_owned(),
            reason: reason.to_owned(),
        },
    )
}

/// The plugin stage is its own stage: classified there and nowhere else, so
/// the stash preserve keeps it and no other stage's clear wipes it.
#[test]
fn plugin_stage_kinds_are_classified_as_plugin_stage_and_nothing_else() {
    for kind in [
        WorkspaceDiagnosticKind::PluginConfigUnreadable {
            plugin: "module-federation".to_owned(),
            key: "exposes".to_owned(),
            reason: "not-object-literal".to_owned(),
        },
        WorkspaceDiagnosticKind::PluginEffectNotModeled {
            plugin: "nuxt".to_owned(),
            key: "components".to_owned(),
            reason: "key-effect-not-modeled".to_owned(),
        },
    ] {
        let id = kind.id();
        assert!(kind.is_plugin_stage(), "{id} must be plugin-stage");
        assert!(!kind.is_analysis_stage(), "{id} must not be analysis-stage");
        assert!(!kind.is_health_stage(), "{id} must not be health-stage");
        assert!(
            !kind.is_source_discovery(),
            "{id} must not be source-discovery"
        );
        assert!(
            !kind.is_source_walk_recorded(),
            "{id} must not be walk-recorded"
        );
        assert!(
            !kind.source_never_analyzed(),
            "{id} reports a config file, not an unread source file"
        );
    }
    assert!(
        !WorkspaceDiagnosticKind::UnpinnedClock.is_plugin_stage(),
        "another stage's kind must not answer the plugin predicate"
    );
}

/// An unread declaration costs findings in both directions, so it degrades
/// the analysis; an effect fallow does not model suppresses findings the
/// user opted into and must not warn on every run forever.
#[test]
fn only_the_unreadable_plugin_config_degrades_the_analysis() {
    let unreadable = plugin_unreadable("exposes", "not-object-literal");
    assert!(
        unreadable.degrades_analysis,
        "an unread declaration did not reach the analysis: {}",
        unreadable.message
    );
    let not_modeled = plugin_not_modeled("components", "key-effect-not-modeled");
    assert!(
        !not_modeled.degrades_analysis,
        "the config was readable and the patterns stayed: {}",
        not_modeled.message
    );
}

/// The reason decides the remedy, so each token renders its own situation,
/// and a token from a later release still renders a sentence.
#[test]
fn every_unreadable_reason_renders_its_own_situation() {
    let cases = [
        ("not-object-literal", "is not a static object literal"),
        ("array-form", "uses the array form"),
        ("spread", "spreads a value that is not statically readable"),
        (
            "unreadable-entries",
            "has entries that hold no statically readable value",
        ),
        (
            "unrecognized-call",
            "is passed through a call that is not a known config wrapper",
        ),
        (
            "import-target-unreadable",
            "comes from an imported file that is not statically readable",
        ),
        (
            "dynamic-argument",
            "receives an argument that is not a static literal",
        ),
    ];
    for (reason, expected) in cases {
        let diagnostic = plugin_unreadable("exposes", reason);
        assert!(
            diagnostic.message.contains(expected),
            "`{reason}` must render its own situation: {}",
            diagnostic.message
        );
    }
    let unknown = plugin_unreadable("exposes", "reason-from-a-later-release");
    assert!(
        unknown.message.contains("could not be read statically"),
        "an unrecognised token still renders a sentence: {}",
        unknown.message
    );
}

/// The payload carries no prose, so the remedy comes from the key: both
/// Module Federation keys name the option that covers the gap, and the
/// message names the config file the user must edit.
#[test]
fn unreadable_plugin_messages_name_the_config_file_and_the_option() {
    let exposes = plugin_unreadable("exposes", "not-object-literal");
    assert!(
        exposes
            .message
            .contains("`exposes` in 'module-federation.config.ts'")
            && exposes.message.contains("dynamicallyLoaded")
            && exposes.message.starts_with("Plugin 'module-federation':"),
        "{}",
        exposes.message
    );
    let remotes = plugin_unreadable("remotes", "spread");
    assert!(
        remotes.message.contains("ignoreDependencies"),
        "the two keys have different remedies: {}",
        remotes.message
    );
    let unknown_key = plugin_unreadable("shared", "not-object-literal");
    assert!(
        !unknown_key.message.contains("dynamicallyLoaded")
            && !unknown_key.message.contains("ignoreDependencies")
            && unknown_key
                .message
                .contains("Declare the value as a static object literal."),
        "a key with no documented consequence falls back to the general claim: {}",
        unknown_key.message
    );
    assert!(
        !exposes.message.contains('\n'),
        "the sentence travels into a CI annotation and stays on one line: {}",
        exposes.message
    );
}

/// An unrecognized call was read as a lower bound, and an unreadable import
/// target holds config that is shared across files. Each renders its own
/// consequence and a remedy that names the option, never an object literal.
#[test]
fn the_call_and_import_reasons_render_their_own_remedy() {
    let call = plugin_unreadable("exposes", "unrecognized-call");
    assert!(
        call.message
            .contains("only the targets in the object literal it receives")
            && call
                .message
                .contains("Name any other exposed files in `dynamicallyLoaded`."),
        "{}",
        call.message
    );
    let call = plugin_unreadable("remotes", "unrecognized-call");
    assert!(
        call.message
            .contains("Name any other aliases in `ignoreDependencies`."),
        "{}",
        call.message
    );
    for key in ["exposes", "remotes"] {
        let import = plugin_unreadable(key, "import-target-unreadable");
        assert!(
            !import.message.contains("object literal"),
            "an import target is not fixed by writing an object literal: {}",
            import.message
        );
    }
    let import = plugin_unreadable("exposes", "import-target-unreadable");
    assert!(
        import.message.contains("`dynamicallyLoaded`"),
        "{}",
        import.message
    );
}

/// A runtime call with a dynamic argument names the source file and the
/// function, and its remedy names the option that covers the remote.
#[test]
fn a_dynamic_runtime_call_renders_its_own_remedy() {
    for (key, consequence) in [
        (
            "registerRemotes",
            "the remotes it registers are not treated as provided",
        ),
        (
            "loadRemote",
            "the remote it loads is not treated as provided",
        ),
        (
            "init",
            "the remotes its options declare are not treated as provided",
        ),
        (
            "createInstance",
            "the remotes its options declare are not treated as provided",
        ),
    ] {
        let diagnostic = plugin_unreadable(key, "dynamic-argument");
        assert!(
            diagnostic.message.contains(&format!("`{key}` in"))
                && diagnostic.message.contains(consequence)
                && diagnostic.message.contains("`ignoreDependencies`")
                && !diagnostic.message.contains("object literal"),
            "{}",
            diagnostic.message
        );
    }
}

/// One config file can hold two unreadable keys, and the payload is what
/// tells them apart: the fold keys on the whole kind, so both survive.
#[test]
fn two_unreadable_keys_in_one_file_are_two_diagnostics() {
    let merged = dedupe_workspace_diagnostics(vec![
        plugin_unreadable("exposes", "not-object-literal"),
        plugin_unreadable("remotes", "spread"),
    ]);
    assert_eq!(merged.len(), 2, "{merged:?}");
}

/// A surface whose own key fallow cannot model and a config file whose
/// top-level property it cannot read need different remedies. The two tokens
/// render different causes, and each cause states one fact per sentence.
#[test]
fn the_not_modeled_reasons_render_different_causes() {
    let key = plugin_not_modeled("components", "key-effect-not-modeled");
    assert!(
        key.message
            .contains("fallow does not model the effect of `components` in 'nuxt.config.ts'.")
            && key
                .message
                .contains("`autoImports` kept the convention entry patterns"),
        "{}",
        key.message
    );
    let property = plugin_not_modeled("imports", "config-property-unreadable");
    assert!(
        property.message.contains(
            "fallow cannot read a top-level property in 'nuxt.config.ts', so it cannot \
             classify the `imports` surface. `autoImports` kept the convention entry patterns \
             for that surface, and fallow reports no unused file there."
        ),
        "{}",
        property.message
    );
    assert_eq!(
        property.message.matches(", so ").count(),
        1,
        "one cause per sentence: {}",
        property.message
    );
}

/// A file that reads a whole virtual module names the file and the module,
/// not a config key, and gives a remedy in the source, not in the config.
#[test]
fn an_unreadable_virtual_module_read_names_the_module_and_a_source_remedy() {
    let read = WorkspaceDiagnostic::new(
        Path::new("/project"),
        PathBuf::from("/project/app/lib/registry.ts"),
        WorkspaceDiagnosticKind::PluginEffectNotModeled {
            plugin: "nuxt".to_owned(),
            key: "#components".to_owned(),
            reason: "key-effect-not-modeled".to_owned(),
        },
    );
    assert!(
        read.message.contains(
            "fallow cannot read which names 'app/lib/registry.ts' takes from `#components`"
        ) && read.message.contains("member access")
            && !read.message.contains("entry patterns"),
        "{}",
        read.message
    );
}
