use super::*;
use std::path::{Path, PathBuf};

#[test]
fn format_bytes_pivots_at_power_of_1024() {
    assert_eq!(format_bytes(0), "0 B");
    assert_eq!(format_bytes(1023), "1023 B");
    assert_eq!(format_bytes(1024), "1 KiB");
    assert_eq!(format_bytes(2048), "2 KiB");
    assert_eq!(format_bytes(1_048_576), "1.0 MiB");
    assert_eq!(format_bytes(10_485_760), "10.0 MiB");
    assert_eq!(format_bytes(1_073_741_824), "1.0 GiB");
}

fn test_context<'a>(root: &'a Path, rules: &'a RulesConfig) -> ReportContext<'a> {
    ReportContext {
        package_baselines: &[],
        baseline_staleness: None,
        finding_id_query: None,
        gate_outcomes: None,
        failed_parse_files: 0,
        root,
        rules,
        workspace_diagnostics: &[],
        elapsed: Duration::default(),
        quiet: true,
        explain: false,
        type_aware: None,
        type_aware_scope: None,
        group_by: None,
        top: None,
        summary: false,
        summary_heading: false,
        show_explain_tip: false,
        baseline_matched: None,
        config_fixable: false,
        skip_score_and_trend: false,
        css_requested: false,
        json_style: crate::json_style::JsonStyle::Compact,
        include_fragments: true,
        architecture_layout: ArchitectureLayout::Embedded,
    }
}

#[test]
fn normalize_uri_forward_slashes_unchanged() {
    assert_eq!(normalize_uri("src/utils.ts"), "src/utils.ts");
}

#[test]
fn normalize_uri_backslashes_replaced() {
    assert_eq!(normalize_uri("src\\utils\\index.ts"), "src/utils/index.ts");
}

#[test]
fn normalize_uri_mixed_slashes() {
    assert_eq!(normalize_uri("src\\utils/index.ts"), "src/utils/index.ts");
}

#[test]
fn normalize_uri_path_with_spaces() {
    assert_eq!(
        normalize_uri("src\\my folder\\file.ts"),
        "src/my folder/file.ts"
    );
}

#[test]
fn normalize_uri_empty_string() {
    assert_eq!(normalize_uri(""), "");
}

#[test]
fn relative_path_strips_root_prefix() {
    let root = Path::new("/project");
    let path = Path::new("/project/src/utils.ts");
    assert_eq!(relative_path(path, root), Path::new("src/utils.ts"));
}

#[test]
fn relative_path_returns_full_path_when_no_prefix() {
    let root = Path::new("/other");
    let path = Path::new("/project/src/utils.ts");
    assert_eq!(relative_path(path, root), path);
}

#[test]
fn relative_path_at_root_returns_empty_or_file() {
    let root = Path::new("/project");
    let path = Path::new("/project/file.ts");
    assert_eq!(relative_path(path, root), Path::new("file.ts"));
}

#[test]
fn relative_path_deeply_nested() {
    let root = Path::new("/project");
    let path = Path::new("/project/packages/ui/src/components/Button.tsx");
    assert_eq!(
        relative_path(path, root),
        Path::new("packages/ui/src/components/Button.tsx")
    );
}

#[test]
fn format_display_path_returns_workspace_relative() {
    let root = Path::new("/project");
    let path = Path::new("/project/apps/server/src/index.ts");
    assert_eq!(format_display_path(path, root), "apps/server/src/index.ts");
}

#[test]
fn format_display_path_collides_in_nx_layout_renders_full_relative() {
    let root = Path::new("/project");
    let server = Path::new("/project/apps/server/src/index.ts");
    let client = Path::new("/project/apps/client/src/index.ts");
    assert_eq!(
        format_display_path(server, root),
        "apps/server/src/index.ts"
    );
    assert_eq!(
        format_display_path(client, root),
        "apps/client/src/index.ts"
    );
}

#[test]
fn format_display_path_angular_component_renders_parent_directory() {
    let root = Path::new("/project");
    let path =
        Path::new("/project/apps/admin/src/app/payments/payment-list/payment-list.component.html");
    assert_eq!(
        format_display_path(path, root),
        "apps/admin/src/app/payments/payment-list/payment-list.component.html"
    );
}

#[test]
fn format_display_path_falls_back_to_full_path_when_root_does_not_prefix() {
    let root = Path::new("/other");
    let path = Path::new("/project/src/utils.ts");
    let rendered = format_display_path(path, root);
    assert!(rendered.contains("project"));
    assert!(rendered.ends_with("utils.ts"));
    assert!(!rendered.contains('\\'));
}

#[test]
fn format_display_path_normalizes_backslashes_to_forward_slashes() {
    let root = Path::new("/project");
    let path = Path::new("/project/src/sub\\file.ts");
    let rendered = format_display_path(path, root);
    assert!(
        !rendered.contains('\\'),
        "backslashes must be normalized: {rendered}"
    );
}

#[test]
fn format_display_path_handles_brackets_verbatim() {
    let root = Path::new("/project");
    let path = Path::new("/project/app/[slug]/page.tsx");
    assert_eq!(format_display_path(path, root), "app/[slug]/page.tsx");
}

#[test]
fn format_display_path_path_equals_root_returns_empty() {
    let root = Path::new("/project");
    let path = Path::new("/project");
    assert_eq!(format_display_path(path, root), "");
}

#[test]
fn format_display_path_basename_only_when_path_is_at_root() {
    let root = Path::new("/project");
    let path = Path::new("/project/Cargo.toml");
    assert_eq!(format_display_path(path, root), "Cargo.toml");
}

#[test]
fn relative_uri_produces_forward_slash_path() {
    let root = PathBuf::from("/project");
    let path = root.join("src").join("utils.ts");
    let uri = relative_uri(&path, &root);
    assert_eq!(uri, "src/utils.ts");
}

#[test]
fn relative_uri_encodes_brackets() {
    let root = PathBuf::from("/project");
    let path = root.join("src/app/[...slug]/page.tsx");
    let uri = relative_uri(&path, &root);
    assert_eq!(uri, "src/app/%5B...slug%5D/page.tsx");
}

#[test]
fn relative_uri_encodes_nested_dynamic_routes() {
    let root = PathBuf::from("/project");
    let path = root.join("src/app/[slug]/[id]/page.tsx");
    let uri = relative_uri(&path, &root);
    assert_eq!(uri, "src/app/%5Bslug%5D/%5Bid%5D/page.tsx");
}

#[test]
fn relative_uri_no_common_prefix_returns_full() {
    let root = PathBuf::from("/other");
    let path = PathBuf::from("/project/src/utils.ts");
    let uri = relative_uri(&path, &root);
    assert!(uri.contains("project"));
    assert!(uri.contains("utils.ts"));
}

#[test]
fn severity_error_maps_to_level_error() {
    assert!(matches!(severity_to_level(Severity::Error), Level::Error));
}

#[test]
fn severity_warn_maps_to_level_warn() {
    assert!(matches!(severity_to_level(Severity::Warn), Level::Warn));
}

#[test]
fn severity_off_maps_to_level_info() {
    assert!(matches!(severity_to_level(Severity::Off), Level::Info));
}

#[test]
fn normalize_uri_single_bracket_pair() {
    assert_eq!(normalize_uri("app/[id]/page.tsx"), "app/%5Bid%5D/page.tsx");
}

#[test]
fn normalize_uri_catch_all_route() {
    assert_eq!(
        normalize_uri("app/[...slug]/page.tsx"),
        "app/%5B...slug%5D/page.tsx"
    );
}

#[test]
fn normalize_uri_optional_catch_all_route() {
    assert_eq!(
        normalize_uri("app/[[...slug]]/page.tsx"),
        "app/%5B%5B...slug%5D%5D/page.tsx"
    );
}

#[test]
fn normalize_uri_multiple_dynamic_segments() {
    assert_eq!(
        normalize_uri("app/[lang]/posts/[id]"),
        "app/%5Blang%5D/posts/%5Bid%5D"
    );
}

#[test]
fn normalize_uri_no_special_chars() {
    let plain = "src/components/Button.tsx";
    assert_eq!(normalize_uri(plain), plain);
}

#[test]
fn normalize_uri_only_backslashes() {
    assert_eq!(normalize_uri("a\\b\\c"), "a/b/c");
}

#[test]
fn relative_path_identical_paths_returns_empty() {
    let root = Path::new("/project");
    assert_eq!(relative_path(root, root), Path::new(""));
}

#[test]
fn relative_path_partial_name_match_not_stripped() {
    let root = Path::new("/project");
    let path = Path::new("/project-two/src/a.ts");
    assert_eq!(relative_path(path, root), path);
}

#[test]
fn relative_uri_combines_stripping_and_encoding() {
    let root = PathBuf::from("/project");
    let path = root.join("src/app/[slug]/page.tsx");
    let uri = relative_uri(&path, &root);
    assert_eq!(uri, "src/app/%5Bslug%5D/page.tsx");
    assert!(!uri.starts_with('/'));
}

#[test]
fn relative_uri_at_root_file() {
    let root = PathBuf::from("/project");
    let path = root.join("index.ts");
    assert_eq!(relative_uri(&path, &root), "index.ts");
}

#[test]
fn severity_to_level_is_const_evaluable() {
    const LEVEL_FROM_ERROR: Level = severity_to_level(Severity::Error);
    const LEVEL_FROM_WARN: Level = severity_to_level(Severity::Warn);
    const LEVEL_FROM_OFF: Level = severity_to_level(Severity::Off);
    assert!(matches!(LEVEL_FROM_ERROR, Level::Error));
    assert!(matches!(LEVEL_FROM_WARN, Level::Warn));
    assert!(matches!(LEVEL_FROM_OFF, Level::Info));
}

#[test]
fn level_is_copy() {
    let level = severity_to_level(Severity::Error);
    let copy = level;
    assert!(matches!(level, Level::Error));
    assert!(matches!(copy, Level::Error));
}

#[test]
fn print_results_rejects_badge_for_dead_code_reports() {
    let root = Path::new("/project");
    let rules = RulesConfig::default();
    let ctx = test_context(root, &rules);

    let code = print_results(&AnalysisResults::default(), &ctx, OutputFormat::Badge, None);

    assert_eq!(code, exit_invalid_input());
}

#[test]
fn print_duplication_report_rejects_badge_format() {
    let root = Path::new("/project");
    let rules = RulesConfig::default();
    let ctx = test_context(root, &rules);

    let code = print_duplication_report(&DuplicationReport::default(), &ctx, OutputFormat::Badge);

    assert_eq!(code, exit_invalid_input());
}

#[test]
fn elide_common_prefix_shared_dir() {
    assert_eq!(
        elide_common_prefix("src/components/A.tsx", "src/components/B.tsx"),
        "B.tsx"
    );
}

#[test]
fn elide_common_prefix_partial_shared() {
    assert_eq!(
        elide_common_prefix("src/components/A.tsx", "src/utils/B.tsx"),
        "utils/B.tsx"
    );
}

#[test]
fn elide_common_prefix_no_shared() {
    assert_eq!(
        elide_common_prefix("pkg-a/src/A.tsx", "pkg-b/src/B.tsx"),
        "pkg-b/src/B.tsx"
    );
}

#[test]
fn elide_common_prefix_identical_files() {
    assert_eq!(elide_common_prefix("a/b/x.ts", "a/b/y.ts"), "y.ts");
}

#[test]
fn elide_common_prefix_no_dirs() {
    assert_eq!(elide_common_prefix("foo.ts", "bar.ts"), "bar.ts");
}

#[test]
fn elide_common_prefix_deep_monorepo() {
    assert_eq!(
        elide_common_prefix(
            "packages/rap/src/rap/components/SearchSelect/SearchSelect.tsx",
            "packages/rap/src/rap/components/SearchSelect/SearchSelectItem.tsx"
        ),
        "SearchSelectItem.tsx"
    );
}

#[test]
fn split_dir_filename_with_dir() {
    let (dir, file) = split_dir_filename("src/utils/index.ts");
    assert_eq!(dir, "src/utils/");
    assert_eq!(file, "index.ts");
}

#[test]
fn split_dir_filename_no_dir() {
    let (dir, file) = split_dir_filename("file.ts");
    assert_eq!(dir, "");
    assert_eq!(file, "file.ts");
}

#[test]
fn split_dir_filename_deeply_nested() {
    let (dir, file) = split_dir_filename("a/b/c/d/e.ts");
    assert_eq!(dir, "a/b/c/d/");
    assert_eq!(file, "e.ts");
}

#[test]
fn split_dir_filename_trailing_slash() {
    let (dir, file) = split_dir_filename("src/");
    assert_eq!(dir, "src/");
    assert_eq!(file, "");
}

#[test]
fn split_dir_filename_empty() {
    let (dir, file) = split_dir_filename("");
    assert_eq!(dir, "");
    assert_eq!(file, "");
}

#[test]
fn plural_zero_is_plural() {
    assert_eq!(plural(0), "s");
}

#[test]
fn plural_one_is_singular() {
    assert_eq!(plural(1), "");
}

#[test]
fn plural_two_is_plural() {
    assert_eq!(plural(2), "s");
}

#[test]
fn plural_large_number() {
    assert_eq!(plural(999), "s");
}

#[test]
fn elide_common_prefix_empty_base() {
    assert_eq!(elide_common_prefix("", "src/foo.ts"), "src/foo.ts");
}

#[test]
fn elide_common_prefix_empty_target() {
    assert_eq!(elide_common_prefix("src/foo.ts", ""), "");
}

#[test]
fn elide_common_prefix_both_empty() {
    assert_eq!(elide_common_prefix("", ""), "");
}

#[test]
fn elide_common_prefix_same_file_different_extension() {
    assert_eq!(
        elide_common_prefix("src/utils.ts", "src/utils.js"),
        "utils.js"
    );
}

#[test]
fn elide_common_prefix_partial_filename_match_not_stripped() {
    assert_eq!(
        elide_common_prefix("src/App.tsx", "src/AppUtils.tsx"),
        "AppUtils.tsx"
    );
}

#[test]
fn elide_common_prefix_identical_paths() {
    assert_eq!(elide_common_prefix("src/foo.ts", "src/foo.ts"), "foo.ts");
}

#[test]
fn split_dir_filename_single_slash() {
    let (dir, file) = split_dir_filename("/file.ts");
    assert_eq!(dir, "/");
    assert_eq!(file, "file.ts");
}

#[test]
fn emit_json_returns_success_for_valid_value() {
    let value = serde_json::json!({"key": "value"});
    let code = emit_json(&value, "test");
    assert_eq!(code, ExitCode::SUCCESS);
}

mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// split_dir_filename always reconstructs the original path.
        #[test]
        fn split_dir_filename_reconstructs_path(path in "[a-zA-Z0-9_./\\-]{0,100}") {
            let (dir, file) = split_dir_filename(&path);
            let reconstructed = format!("{dir}{file}");
            prop_assert_eq!(
                reconstructed, path,
                "dir+file should reconstruct the original path"
            );
        }

        /// plural returns either "" or "s", nothing else.
        #[test]
        fn plural_returns_empty_or_s(n: usize) {
            let result = plural(n);
            prop_assert!(
                result.is_empty() || result == "s",
                "plural should return \"\" or \"s\", got {:?}",
                result
            );
        }

        /// plural(1) is always "" and plural(n != 1) is always "s".
        #[test]
        fn plural_singular_only_for_one(n: usize) {
            let result = plural(n);
            if n == 1 {
                prop_assert_eq!(result, "", "plural(1) should be empty");
            } else {
                prop_assert_eq!(result, "s", "plural({}) should be \"s\"", n);
            }
        }

        /// normalize_uri never panics and always replaces backslashes.
        #[test]
        fn normalize_uri_no_backslashes(path in "[a-zA-Z0-9_.\\\\/ \\[\\]%-]{0,100}") {
            let result = normalize_uri(&path);
            prop_assert!(
                !result.contains('\\'),
                "Result should not contain backslashes: {result}"
            );
        }

        /// normalize_uri always encodes brackets.
        #[test]
        fn normalize_uri_encodes_all_brackets(path in "[a-zA-Z0-9_./\\[\\]%-]{0,80}") {
            let result = normalize_uri(&path);
            prop_assert!(
                !result.contains('[') && !result.contains(']'),
                "Result should not contain raw brackets: {result}"
            );
        }

        /// elide_common_prefix always returns a suffix of or equal to target.
        #[test]
        fn elide_common_prefix_returns_suffix_of_target(
            base in "[a-zA-Z0-9_./]{0,50}",
            target in "[a-zA-Z0-9_./]{0,50}",
        ) {
            let result = elide_common_prefix(&base, &target);
            prop_assert!(
                target.ends_with(result),
                "Result {:?} should be a suffix of target {:?}",
                result, target
            );
        }

        /// relative_path never panics.
        #[test]
        fn relative_path_never_panics(
            root in "/[a-zA-Z0-9_/]{0,30}",
            suffix in "[a-zA-Z0-9_./]{0,30}",
        ) {
            let root_path = Path::new(&root);
            let full = PathBuf::from(format!("{root}/{suffix}"));
            let _ = relative_path(&full, root_path);
        }
    }
}
