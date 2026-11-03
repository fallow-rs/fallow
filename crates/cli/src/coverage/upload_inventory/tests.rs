use super::*;
use crate::coverage::COVERAGE_UPLOAD_VALIDATION_EXIT_CODE as EXIT_VALIDATION;
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

#[test]
fn upload_inventory_args_debug_masks_api_key() {
    let args = UploadInventoryArgs {
        api_key: Some("fallow_live_secret_token_value".to_owned()),
        api_endpoint: Some("https://api.fallow.cloud".to_owned()),
        project_id: Some("acme/web".to_owned()),
        ..UploadInventoryArgs::default()
    };
    let formatted = format!("{args:?}");
    assert!(
        !formatted.contains("fallow_live_secret_token_value"),
        "api_key leaked through Debug: {formatted}"
    );
    assert!(
        formatted.contains("api_key: Some(\"***\")"),
        "expected explicit redaction marker, got: {formatted}"
    );
    let bare = UploadInventoryArgs::default();
    let formatted_bare = format!("{bare:?}");
    assert!(
        formatted_bare.contains("api_key: None"),
        "expected None for unset api_key, got: {formatted_bare}"
    );
}

#[test]
fn compile_exclude_matcher_rejects_invalid_glob() {
    let err = compile_exclude_matcher(&["[".to_owned()])
        .expect_err("invalid glob should be reported as validation");

    let UploadError::Validation(message) = err else {
        panic!("expected validation error, got {err:?}");
    };
    assert!(message.contains("invalid --exclude-paths"));
}

#[test]
fn collect_inventory_applies_path_prefix_and_excludes() {
    let project = project_with_one_function();
    let config = upload_common::load_resolved_config(project.path()).unwrap();
    let session = AnalysisSession::from_resolved_config(config).expect("session");
    let include_all = compile_exclude_matcher(&[]).unwrap();

    let functions = collect_inventory(&session, &include_all, Some("/app"));

    assert_eq!(functions.len(), 1);
    assert_eq!(functions[0].file_path, "/app/src/index.ts");
    assert_eq!(functions[0].identity.file, "src/index.ts");

    let exclude_src = compile_exclude_matcher(&["src/**".to_owned()]).unwrap();
    assert!(collect_inventory(&session, &exclude_src, Some("/app")).is_empty());
}

#[test]
fn normalize_path_prefix_rejects_empty() {
    assert!(matches!(
        normalize_path_prefix(Some("")),
        Err(UploadError::Validation(_))
    ));
    assert!(matches!(
        normalize_path_prefix(Some("   ")),
        Err(UploadError::Validation(_))
    ));
}

#[test]
fn normalize_path_prefix_rejects_backslash() {
    assert!(matches!(
        normalize_path_prefix(Some("\\app")),
        Err(UploadError::Validation(_))
    ));
    assert!(matches!(
        normalize_path_prefix(Some("/home\\runner")),
        Err(UploadError::Validation(_))
    ));
}

#[test]
fn normalize_path_prefix_rejects_relative() {
    assert!(matches!(
        normalize_path_prefix(Some("app")),
        Err(UploadError::Validation(_))
    ));
    assert!(matches!(
        normalize_path_prefix(Some("./app")),
        Err(UploadError::Validation(_))
    ));
}

#[test]
fn normalize_path_prefix_accepts_absolute_posix() {
    assert_eq!(
        normalize_path_prefix(Some("/app")).unwrap(),
        Some("/app".to_owned())
    );
    assert_eq!(
        normalize_path_prefix(Some("/home/runner/work/my-repo/my-repo")).unwrap(),
        Some("/home/runner/work/my-repo/my-repo".to_owned())
    );
}

#[test]
fn normalize_path_prefix_trims_trailing_slash_and_whitespace() {
    assert_eq!(
        normalize_path_prefix(Some("/app/")).unwrap(),
        Some("/app".to_owned())
    );
    assert_eq!(
        normalize_path_prefix(Some("  /workspace/  ")).unwrap(),
        Some("/workspace".to_owned())
    );
}

#[test]
fn normalize_path_prefix_none_stays_none() {
    assert_eq!(normalize_path_prefix(None).unwrap(), None);
}

#[test]
fn count_digits_matches_base10_length() {
    assert_eq!(count_digits(0), 1);
    assert_eq!(count_digits(1), 1);
    assert_eq!(count_digits(9), 1);
    assert_eq!(count_digits(10), 2);
    assert_eq!(count_digits(99), 2);
    assert_eq!(count_digits(100), 3);
    assert_eq!(count_digits(9_999), 4);
}

#[test]
fn extension_supported_handles_all_js_ts_variants() {
    for ext in ["js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "cts"] {
        let path = PathBuf::from(format!("a.{ext}"));
        assert!(extension_supported(&path), "missing support for .{ext}");
    }
}

#[test]
fn extension_supported_rejects_non_js_ts() {
    for ext in ["md", "json", "css", "html", "vue", "svelte", "astro", "mdx"] {
        let path = PathBuf::from(format!("a.{ext}"));
        assert!(!extension_supported(&path), ".{ext} must be skipped in v1");
    }
}

#[test]
fn extension_supported_skips_typescript_declaration_files() {
    for name in [
        "types.d.ts",
        "client.d.ts",
        "Index.D.TS",
        "lib.d.mts",
        "a.d.cts",
        "b.d.tsx",
    ] {
        let path = PathBuf::from(name);
        assert!(
            !extension_supported(&path),
            "{name} should be skipped as a declaration file"
        );
    }
}

#[test]
fn extension_supported_still_accepts_non_declaration_ts() {
    for name in ["vite.config.ts", "file.weird.d.name.ts"] {
        let path = PathBuf::from(name);
        assert!(extension_supported(&path), "{name} should still be walked");
    }
}

#[test]
fn classify_upload_error_maps_400_payload_too_large_to_dedicated_exit() {
    let err = classify_upload_error(400, Some("payload_too_large"), "stub".to_owned())
        .expect_err("400 must error");
    assert!(matches!(err, UploadError::PayloadTooLarge(_)));
}

#[test]
fn classify_upload_error_falls_back_to_validation_on_other_400_codes() {
    let err = classify_upload_error(400, Some("bad_request"), "stub".to_owned())
        .expect_err("400 must error");
    assert!(matches!(err, UploadError::Validation(_)));
    let err = classify_upload_error(400, None, "stub".to_owned())
        .expect_err("400 with no code must error");
    assert!(matches!(err, UploadError::Validation(_)));
}

#[test]
fn classify_upload_error_maps_auth_codes_to_auth_rejected() {
    for status in [401, 403] {
        let err = classify_upload_error(status, Some("unauthorized"), "stub".to_owned())
            .expect_err("auth status must error");
        assert!(
            matches!(err, UploadError::AuthRejected(_)),
            "status={status}"
        );
    }
}

#[test]
fn classify_upload_error_maps_5xx_to_server_error() {
    for status in [500, 502, 503, 504] {
        let err =
            classify_upload_error(status, None, "stub".to_owned()).expect_err("5xx must error");
        assert!(
            matches!(err, UploadError::ServerError(_)),
            "status={status}"
        );
    }
}

#[test]
fn format_upload_error_message_uses_hint_for_known_code() {
    let envelope = parse_error_envelope(r#"{"code":"payload_too_large"}"#);
    let message = format_upload_error_message(
        "upload-inventory",
        400,
        "{}",
        Some("payload_too_large"),
        &envelope,
    );
    assert!(
        message.contains("200,000-function server limit"),
        "got: {message}"
    );
    assert!(message.contains("HTTP 400"));
    assert!(message.contains("code payload_too_large"));
}

#[test]
fn format_upload_error_message_falls_back_to_server_message() {
    let body = r#"{"code":"internal","message":"database timeout"}"#;
    let envelope = parse_error_envelope(body);
    let message =
        format_upload_error_message("upload-inventory", 500, body, Some("internal"), &envelope);
    assert!(message.starts_with("upload-inventory request failed with HTTP 500"));
    assert!(message.ends_with(": database timeout"));
}

#[test]
fn format_upload_error_message_handles_empty_body() {
    let envelope = parse_error_envelope("");
    let message = format_upload_error_message("upload-inventory", 502, "", None, &envelope);
    assert_eq!(message, "upload-inventory request failed with HTTP 502");
}

#[test]
fn format_upload_error_message_preserves_malformed_body() {
    let body = "gateway timeout";
    let envelope = parse_error_envelope(body);
    let message = format_upload_error_message("upload-inventory", 500, body, None, &envelope);
    assert!(message.contains("gateway timeout"));
    assert!(message.contains("malformed error envelope"));
}

fn run_git(root: &Path, args: &[&str]) {
    let status = fallow_engine::changed_files::clear_ambient_git_env(&mut Command::new("git"))
        .args(args)
        .current_dir(root)
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?} failed");
}

fn sample_entry() -> InventoryEntry {
    InventoryEntry {
        name: "render".to_owned(),
        line: 42,
        start_column: 1,
        end_line: 50,
        end_column: 2,
        source_hash: Some("0123456789abcdef".to_owned()),
        is_callback: false,
    }
}

#[test]
fn identity_stable_id_is_repo_relative_not_prefixed() {
    let func = InventoryFunction::from_entry(
        "/app/src/render.tsx",
        "src/render.tsx",
        sample_entry(),
        None,
    );
    assert_eq!(func.file_path, "/app/src/render.tsx");
    assert_eq!(func.identity.file, "src/render.tsx");
    assert_eq!(
        func.identity.stable_id,
        function_identity_id("src/render.tsx", "render", 42)
    );
}

#[test]
fn identity_stable_id_unchanged_by_path_prefix() {
    let with_prefix = InventoryFunction::from_entry(
        "/app/src/render.tsx",
        "src/render.tsx",
        sample_entry(),
        None,
    );
    let without_prefix =
        InventoryFunction::from_entry("src/render.tsx", "src/render.tsx", sample_entry(), None);
    assert_ne!(with_prefix.file_path, without_prefix.file_path);
    assert_eq!(
        with_prefix.identity.stable_id,
        without_prefix.identity.stable_id
    );
    assert_eq!(with_prefix.identity.file, without_prefix.identity.file);
}

#[test]
fn identity_matches_protocol_conformance_fixture() {
    assert_eq!(
        function_identity_id("src/render.tsx", "render", 42),
        "fallow:fn:cb4482d6aef7c79a"
    );
}

#[test]
fn inventory_function_emits_resolved_columns() {
    let func = InventoryFunction::from_entry("src/a.ts", "src/a.ts", sample_entry(), None);
    assert_eq!(func.identity.resolution, IdentityResolution::Resolved);
    assert_eq!(func.identity.start_column, Some(1));
    assert_eq!(func.identity.end_line, Some(50));
    assert_eq!(func.identity.end_column, Some(2));
    assert_eq!(
        func.identity.source_hash.as_deref(),
        Some("0123456789abcdef")
    );
}

#[test]
fn inventory_function_carries_complexity_when_paired() {
    let metrics = InventoryComplexity {
        cyclomatic: 7,
        cognitive: 4,
    };
    let func = InventoryFunction::from_entry("src/a.ts", "src/a.ts", sample_entry(), Some(metrics));
    assert_eq!(func.cyclomatic, Some(7));
    assert_eq!(func.cognitive, Some(4));
}

#[test]
fn inventory_function_omits_complexity_when_unpaired() {
    let func = InventoryFunction::from_entry("src/a.ts", "src/a.ts", sample_entry(), None);
    assert_eq!(func.cyclomatic, None);
    assert_eq!(func.cognitive, None);
    // Optional + skip-if-none means the field is absent from the wire form,
    // so an older server and a non-complexity producer stay byte-compatible.
    let json = serde_json::to_string(&func).expect("serialize");
    assert!(
        !json.contains("cyclomatic"),
        "unpaired complexity must not appear on the wire: {json}"
    );
}

#[test]
fn complexity_populates_from_real_walk() {
    // Drive the actual walk+complexity pairing over a branchy function so a
    // regression in the source_hash join surfaces as a missing metric.
    let project = project_with_branchy_function();
    let config = upload_common::load_resolved_config(project.path()).unwrap();
    let session = AnalysisSession::from_resolved_config(config).expect("session");
    let include_all = compile_exclude_matcher(&[]).unwrap();
    let functions = collect_inventory(&session, &include_all, None);
    let branchy = functions
        .iter()
        .find(|f| f.function_name == "branchy")
        .expect("branchy function present");
    let cyclomatic = branchy.cyclomatic.expect("cyclomatic populated from walk");
    assert!(
        cyclomatic >= 3,
        "branchy has multiple decision points, got cyclomatic {cyclomatic}"
    );
}

fn project_with_one_function() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("package.json"), r#"{"name":"inv"}"#).unwrap();
    std::fs::write(
        root.join("src/index.ts"),
        "export function boot() {\n  return 1;\n}\n",
    )
    .unwrap();
    dir
}

fn project_with_branchy_function() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("package.json"), r#"{"name":"inv"}"#).unwrap();
    std::fs::write(
        root.join("src/index.ts"),
        "export function branchy(x: number) {\n  if (x > 0) {\n    return 1;\n  } else if (x < 0) {\n    return -1;\n  }\n  return 0;\n}\n",
    )
    .unwrap();
    dir
}

/// Build a committed single-file git repo so `analyze_churn_cached` has real
/// history to read. Returns the temp dir; the committed file is `src/a.ts`.
fn git_repo_with_history() -> TempDir {
    let dir = tempfile::tempdir().expect("create temp repo");
    let root = dir.path();
    run_git(root, &["init", "-q"]);
    run_git(root, &["config", "commit.gpgsign", "false"]);
    run_git(root, &["config", "user.email", "review@example.com"]);
    run_git(root, &["config", "user.name", "Reviewer"]);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("package.json"), r#"{"name":"inv"}"#).unwrap();
    std::fs::write(
        root.join("src/a.ts"),
        "export function one() {\n  return 1;\n}\n",
    )
    .unwrap();
    run_git(root, &["add", "."]);
    run_git(root, &["commit", "-qm", "first"]);
    std::fs::write(
        root.join("src/a.ts"),
        "export function one() {\n  return 2;\n}\n",
    )
    .unwrap();
    run_git(root, &["add", "."]);
    run_git(root, &["commit", "-qm", "second"]);
    dir
}

#[test]
fn churn_by_path_keys_match_prefixed_file_path() {
    let repo = git_repo_with_history();
    let config = upload_common::load_resolved_config(repo.path()).unwrap();
    let churn = collect_churn(&config, Some("/app"));
    // A committed, twice-edited file must appear keyed by the SAME prefixed
    // posix path the inventory functions carry, so the server can join them.
    let entry = churn
        .get("/app/src/a.ts")
        .expect("committed file present in churn map keyed by prefixed path");
    assert!(
        entry.commits.unwrap_or(0) >= 2,
        "two commits touched the file"
    );
    assert!(entry.author_count.unwrap_or(0) >= 1, "one author present");
    assert!(entry.last_commit_ts.is_some(), "recency timestamp present");
    assert!(entry.trend.is_some(), "trend present");
    // The prefixed inventory path and the churn key must use the same shape.
    let include_all = compile_exclude_matcher(&[]).unwrap();
    let session = AnalysisSession::from_resolved_config(config).expect("session");
    let functions = collect_inventory(&session, &include_all, Some("/app"));
    let a_fn = functions
        .iter()
        .find(|f| f.file_path == "/app/src/a.ts")
        .expect("inventory function for src/a.ts");
    assert!(
        churn.contains_key(&a_fn.file_path),
        "inventory filePath {} must be a churn key",
        a_fn.file_path
    );
}

#[test]
fn churn_is_empty_and_errorless_on_non_git_root() {
    // No `git init`: a plain directory must yield an empty churn map and no
    // error, so the enrichment degrades gracefully off a version-control
    // host.
    let project = project_with_one_function();
    let config = upload_common::load_resolved_config(project.path()).unwrap();
    let churn = collect_churn(&config, None);
    assert!(churn.is_empty(), "non-git root must produce empty churn");
}

#[test]
fn request_serializes_v2_version_and_churn_when_present() {
    let repo = git_repo_with_history();
    let config = upload_common::load_resolved_config(repo.path()).unwrap();
    let session = AnalysisSession::from_resolved_config(config.clone()).expect("session");
    let include_all = compile_exclude_matcher(&[]).unwrap();
    let functions = collect_inventory(&session, &include_all, None);
    let churn_by_path = collect_churn(&config, None);
    let request = InventoryRequest {
        version: INVENTORY_BLOB_VERSION,
        git_sha: "deadbeef",
        functions: &functions,
        churn_by_path,
        caller_edges: BTreeMap::new(),
        caller_edge_limits: None,
        routes: &[],
        route_limits: None,
    };
    let json = serde_json::to_value(&request).expect("serialize request");
    assert_eq!(json["version"], 2, "v2 version field present on the wire");
    assert_eq!(json["gitSha"], "deadbeef");
    assert!(
        json["churnByPath"].is_object(),
        "churnByPath present when git history exists"
    );
    let file_churn = &json["churnByPath"]["src/a.ts"];
    assert!(file_churn["commits"].is_number(), "commits emitted");
    assert!(
        file_churn["weightedCommits"].is_number(),
        "weightedCommits emitted"
    );
    assert!(file_churn["authorCount"].is_number(), "authorCount emitted");
    assert!(
        file_churn["lastCommitTs"].is_number(),
        "lastCommitTs emitted"
    );
    assert!(file_churn["trend"].is_string(), "trend emitted as a string");
}

#[test]
fn request_omits_churn_when_empty() {
    // A v2 request built off a non-git root must not emit `churnByPath` at
    // all, keeping the pre-enrichment wire shape for non-git uploads.
    let functions: Vec<InventoryFunction> = Vec::new();
    let request = InventoryRequest {
        version: INVENTORY_BLOB_VERSION,
        git_sha: "deadbeef",
        functions: &functions,
        churn_by_path: BTreeMap::new(),
        caller_edges: BTreeMap::new(),
        caller_edge_limits: None,
        routes: &[],
        route_limits: None,
    };
    let json = serde_json::to_value(&request).expect("serialize request");
    assert!(
        json.get("churnByPath").is_none(),
        "empty churn must be omitted from the wire"
    );
}

fn dry_run_args() -> UploadInventoryArgs {
    UploadInventoryArgs {
        project_id: Some("acme/web".to_owned()),
        git_sha: Some("abcdef1".to_owned()),
        api_endpoint: Some("http://localhost:3000".to_owned()),
        allow_dirty: true,
        dry_run: true,
        ..UploadInventoryArgs::default()
    }
}

#[test]
fn run_dry_run_emits_inventory_and_exits_zero() {
    let project = project_with_one_function();
    // Explicit project_id + git_sha keep this env- and git-free.
    let code = run(&dry_run_args(), project.path(), false);
    assert_eq!(code, ExitCode::SUCCESS);
}

#[test]
fn run_with_no_functions_is_a_validation_exit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("package.json"), r#"{"name":"inv"}"#).unwrap();
    // Only a declaration file: the walker intentionally skips `*.d.ts`, so
    // the inventory is empty and run_inner returns a validation error.
    std::fs::write(
        root.join("src/types.d.ts"),
        "export declare const x: number;\n",
    )
    .unwrap();
    let code = run(&dry_run_args(), root, false);
    assert_eq!(code, ExitCode::from(EXIT_VALIDATION));
}

fn entry(name: &str, line: u32, hash: &str) -> InventoryEntry {
    InventoryEntry {
        name: name.to_owned(),
        line,
        start_column: 1,
        end_line: line + 1,
        end_column: 2,
        source_hash: Some(hash.to_owned()),
        is_callback: false,
    }
}

#[test]
fn attribute_caller_edges_matches_symbol_to_function_by_name() {
    let foo =
        InventoryFunction::from_entry("src/foo.ts", "src/foo.ts", entry("foo", 3, "h1"), None);
    let bar =
        InventoryFunction::from_entry("src/foo.ts", "src/foo.ts", entry("bar", 8, "h2"), None);
    let functions = vec![foo.clone(), bar.clone()];
    let edge = |importer: &str, symbol: &str| ImporterEdge {
        callee_file: "src/foo.ts".to_owned(),
        importer_file: importer.to_owned(),
        symbol: symbol.to_owned(),
    };
    let edges = vec![
        edge("src/a.ts", "foo"),
        edge("src/b.ts", "foo"),
        edge("src/c.ts", "missing"), // no function named "missing" -> ignored
        edge("src/d.ts", "*"),       // namespace import -> ignored
    ];

    let map = attribute_caller_edges(&functions, &edges);

    assert_eq!(map.truncated_functions, 0, "no fan-in reached the cap");
    let foo_sites = map
        .by_callee
        .get(&foo.identity.stable_id)
        .expect("foo has importer edges");
    assert_eq!(
        foo_sites
            .iter()
            .map(|s| s.file.as_str())
            .collect::<Vec<_>>(),
        vec!["src/a.ts", "src/b.ts"]
    );
    assert_eq!(foo_sites[0].symbols, vec!["foo".to_owned()]);
    // bar is never imported -> absent (never a placeholder entry).
    assert!(!map.by_callee.contains_key(&bar.identity.stable_id));
}

#[test]
fn attribute_caller_edges_dedups_repeated_importer_symbol() {
    let foo =
        InventoryFunction::from_entry("src/foo.ts", "src/foo.ts", entry("foo", 1, "h1"), None);
    let edge = ImporterEdge {
        callee_file: "src/foo.ts".to_owned(),
        importer_file: "src/a.ts".to_owned(),
        symbol: "foo".to_owned(),
    };
    let map = attribute_caller_edges(std::slice::from_ref(&foo), &[edge.clone(), edge]);
    let sites = map
        .by_callee
        .get(&foo.identity.stable_id)
        .expect("foo edges");
    assert_eq!(sites.len(), 1, "same importer+symbol collapses to one site");
    assert_eq!(sites[0].symbols, vec!["foo".to_owned()]);
}

#[test]
fn collect_caller_edges_reuses_session_files_and_graph() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"inv","type":"module"}"#,
    )
    .expect("write package");
    std::fs::write(
        root.join("src/callee.ts"),
        "export function boot() {\n  return 1;\n}\n",
    )
    .expect("write callee");
    std::fs::write(
        root.join("src/index.ts"),
        "import { boot } from './callee';\nboot();\n",
    )
    .expect("write importer");

    let config = upload_common::load_resolved_config(root).expect("config loads");
    let session = AnalysisSession::from_resolved_config(config).expect("session");
    let mut function = InventoryFunction::from_entry(
        "src/callee.ts",
        "src/callee.ts",
        entry("boot", 1, "h1"),
        None,
    );

    let edges = collect_graph_context(&session, std::slice::from_mut(&mut function)).0;

    let sites = edges
        .by_callee
        .get(&function.identity.stable_id)
        .expect("caller edge should be resolved from retained graph/files");
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].file, "src/index.ts");
    assert_eq!(sites[0].symbols, vec!["boot".to_owned()]);
}

/// Fixture project with a known call graph: `src/math.ts` exports a branchy
/// `classify` plus an unimported `unused`, and two modules import
/// `classify`. Deliberately small and fully deterministic so the expected
/// edge list and complexity numbers can be asserted exactly.
fn project_with_known_call_graph() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"inv","type":"module"}"#,
    )
    .expect("write package");
    // cyclomatic 3 (1 + two `if`), cognitive 2 (two flat `if` statements).
    std::fs::write(
        root.join("src/math.ts"),
        "export function classify(x: number) {\n  if (x > 0) {\n    return 1;\n  }\n  if (x < 0) {\n    return -1;\n  }\n  return 0;\n}\n\nexport function unused() {\n  return 0;\n}\n",
    )
    .expect("write callee");
    std::fs::write(
        root.join("src/first.ts"),
        "import { classify } from './math';\nclassify(1);\n",
    )
    .expect("write first importer");
    std::fs::write(
        root.join("src/second.ts"),
        "import { classify } from './math';\nclassify(-1);\n",
    )
    .expect("write second importer");
    dir
}

#[test]
fn fixture_call_graph_round_trips_through_the_blob() {
    let project = project_with_known_call_graph();
    let config = upload_common::load_resolved_config(project.path()).expect("config loads");
    let session = AnalysisSession::from_resolved_config(config).expect("session");
    let include_all = compile_exclude_matcher(&[]).unwrap();
    let mut functions = collect_inventory(&session, &include_all, None);
    let (caller_edges, _) = collect_graph_context(&session, &mut functions);
    let caller_edge_limits = caller_edges.limits();
    let request = InventoryRequest {
        version: INVENTORY_BLOB_VERSION_WITH_CALLERS,
        git_sha: "deadbeef",
        functions: &functions,
        churn_by_path: BTreeMap::new(),
        caller_edges: caller_edges.by_callee,
        caller_edge_limits,
        routes: &[],
        route_limits: None,
    };
    let json = serde_json::to_value(&request).expect("serialize request");

    assert_eq!(json["version"], 3);

    // Complexity travels per function with the exact numbers the health
    // pass computes for this fixture.
    let classify = json["functions"]
        .as_array()
        .expect("functions array")
        .iter()
        .find(|f| f["functionName"] == "classify")
        .expect("classify present");
    assert_eq!(classify["cyclomatic"], 3, "1 + two if statements");
    assert_eq!(classify["cognitive"], 2, "two flat if statements");
    let classify_stable_id = classify["identity"]["stable_id"]
        .as_str()
        .expect("stable_id present")
        .to_owned();

    // The edge list is keyed by the callee stable_id and lists both
    // importers, in deterministic path order.
    let sites = json["callerEdges"][&classify_stable_id]
        .as_array()
        .expect("classify has importer edges");
    let files: Vec<&str> = sites
        .iter()
        .map(|site| site["file"].as_str().expect("file"))
        .collect();
    assert_eq!(files, vec!["src/first.ts", "src/second.ts"]);
    assert_eq!(sites[0]["symbols"][0], "classify");

    // An exported-but-unimported function gets no entry at all, never a
    // placeholder with zero sites.
    let unused = json["functions"]
        .as_array()
        .expect("functions array")
        .iter()
        .find(|f| f["functionName"] == "unused")
        .expect("unused present");
    let unused_stable_id = unused["identity"]["stable_id"]
        .as_str()
        .expect("stable_id present");
    assert!(json["callerEdges"].get(unused_stable_id).is_none());

    // Blob header carries the size guard so a reader can interpret fan-in.
    assert_eq!(
        json["callerEdgeLimits"]["maxSitesPerFunction"],
        MAX_CALLER_SITES_PER_FN
    );
    assert_eq!(
        json["callerEdgeLimits"]["maxSymbolsPerSite"],
        MAX_SYMBOLS_PER_CALLER_SITE
    );
    assert_eq!(json["callerEdgeLimits"]["truncatedFunctions"], 0);
}

#[test]
fn caller_edge_fan_in_is_capped_and_reported_in_the_header() {
    let callee = InventoryFunction::from_entry(
        "src/barrel.ts",
        "src/barrel.ts",
        entry("widely_used", 1, "h1"),
        None,
    );
    let over_cap = MAX_CALLER_SITES_PER_FN + 7;
    let edges: Vec<ImporterEdge> = (0..over_cap)
        .map(|index| ImporterEdge {
            callee_file: "src/barrel.ts".to_owned(),
            importer_file: format!("src/importer-{index:04}.ts"),
            symbol: "widely_used".to_owned(),
        })
        .collect();

    let attributed = attribute_caller_edges(std::slice::from_ref(&callee), &edges);

    let sites = attributed
        .by_callee
        .get(&callee.identity.stable_id)
        .expect("callee edges");
    assert_eq!(sites.len(), MAX_CALLER_SITES_PER_FN, "fan-in is capped");
    assert_eq!(attributed.truncated_functions, 1);
    let limits = attributed.limits().expect("limits present with edges");
    assert_eq!(
        limits.max_sites_per_function,
        u32::try_from(MAX_CALLER_SITES_PER_FN).unwrap()
    );
    assert_eq!(limits.truncated_functions, 1);
}

#[test]
fn caller_edge_fan_in_exactly_at_the_cap_is_not_reported_as_truncated() {
    let callee = InventoryFunction::from_entry(
        "src/barrel.ts",
        "src/barrel.ts",
        entry("widely_used", 1, "h1"),
        None,
    );
    let edges: Vec<ImporterEdge> = (0..MAX_CALLER_SITES_PER_FN)
        .map(|index| ImporterEdge {
            callee_file: "src/barrel.ts".to_owned(),
            importer_file: format!("src/importer-{index:04}.ts"),
            symbol: "widely_used".to_owned(),
        })
        .collect();

    let attributed = attribute_caller_edges(std::slice::from_ref(&callee), &edges);

    assert_eq!(
        attributed
            .by_callee
            .get(&callee.identity.stable_id)
            .expect("callee edges")
            .len(),
        MAX_CALLER_SITES_PER_FN
    );
    assert_eq!(
        attributed.truncated_functions, 0,
        "a fan-in that exactly fills the cap lost nothing"
    );
}

#[test]
fn caller_edge_limits_are_absent_without_caller_edges() {
    // A v1/v2 body (no caller edges) must not grow a header field: an older
    // server validates the pre-enrichment shape byte for byte.
    assert!(CallerEdges::default().limits().is_none());
    let request = InventoryRequest {
        version: INVENTORY_BLOB_VERSION,
        git_sha: "abc",
        functions: &[],
        churn_by_path: BTreeMap::new(),
        caller_edges: BTreeMap::new(),
        caller_edge_limits: CallerEdges::default().limits(),
        routes: &[],
        route_limits: None,
    };
    let json = serde_json::to_value(&request).expect("serialize");
    assert!(json.get("callerEdgeLimits").is_none());
    assert!(json.get("callerEdges").is_none());
}

#[test]
fn caller_edges_serialize_as_camel_case_and_skip_when_empty() {
    let empty = InventoryRequest {
        version: INVENTORY_BLOB_VERSION,
        git_sha: "abc",
        functions: &[],
        churn_by_path: BTreeMap::new(),
        caller_edges: BTreeMap::new(),
        caller_edge_limits: None,
        routes: &[],
        route_limits: None,
    };
    let value = serde_json::to_value(&empty).expect("serialize empty");
    assert!(
        value.get("callerEdges").is_none(),
        "empty caller edges must be omitted, got: {value}"
    );

    let mut caller_edges = BTreeMap::new();
    caller_edges.insert(
        "sid1".to_owned(),
        vec![CallerSitePayload {
            file: "src/a.ts".to_owned(),
            symbols: vec!["foo".to_owned()],
        }],
    );
    let populated = InventoryRequest {
        version: INVENTORY_BLOB_VERSION_WITH_CALLERS,
        git_sha: "abc",
        functions: &[],
        churn_by_path: BTreeMap::new(),
        caller_edges,
        caller_edge_limits: None,
        routes: &[],
        route_limits: None,
    };
    let value = serde_json::to_value(&populated).expect("serialize populated");
    assert_eq!(value["callerEdges"]["sid1"][0]["file"], "src/a.ts");
    assert_eq!(value["callerEdges"]["sid1"][0]["symbols"][0], "foo");
    assert_eq!(value["version"], 3);
}
