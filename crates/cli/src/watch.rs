use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use colored::Colorize;
use fallow_config::OutputFormat;
use notify_debouncer_mini::{DebouncedEventKind, new_debouncer};
use rustc_hash::FxHashSet;

use crate::report;
use crate::runtime_support::load_config;

/// ANSI escape: clear screen + scrollback + move cursor home (same sequence as tsc --watch).
const CLEAR_SCREEN: &str = "\x1B[2J\x1B[3J\x1B[H";

pub struct WatchOptions<'a> {
    pub root: &'a Path,
    pub config_path: &'a Option<PathBuf>,
    pub output: OutputFormat,
    pub no_cache: bool,
    pub threads: usize,
    pub quiet: bool,
    pub production: bool,
    pub clear_screen: bool,
    pub explain: bool,
    /// Mirror of the global `--include-entry-exports` flag. When true, ORs into the
    /// loaded config's `include_entry_exports` field so the CLI flag also takes
    /// effect under watch mode (config-file-driven `includeEntryExports: true`
    /// already worked through plain config loading).
    pub include_entry_exports: bool,
}

type LoadConfigFn = fn(
    root: &Path,
    config_path: &Option<PathBuf>,
    output: OutputFormat,
    no_cache: bool,
    threads: usize,
    production: bool,
    quiet: bool,
) -> Result<fallow_config::ResolvedConfig, ExitCode>;

fn is_relevant_source(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| fallow_core::discover::SOURCE_EXTENSIONS.contains(&ext))
}

fn is_relevant_config(path: &Path) -> bool {
    path.file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|name| {
            matches!(
                name,
                "package.json"
                    | ".fallowrc.json"
                    | ".fallowrc.jsonc"
                    | "fallow.toml"
                    | ".fallow.toml"
                    | "tsconfig.json"
            )
        })
}

/// Collect changed file paths from debounced events, deduplicating and stripping the root prefix.
fn collect_changed_paths(
    events: &[notify_debouncer_mini::DebouncedEvent],
    root: &Path,
) -> Vec<String> {
    let mut seen = FxHashSet::default();
    let mut paths = Vec::new();
    for event in events {
        if !matches!(event.kind, DebouncedEventKind::Any) {
            continue;
        }
        if !is_relevant_source(&event.path) && !is_relevant_config(&event.path) {
            continue;
        }
        let display = event
            .path
            .strip_prefix(root)
            .unwrap_or(&event.path)
            .display()
            .to_string();
        if seen.insert(display.clone()) {
            paths.push(display);
        }
    }
    paths
}

fn print_waiting() {
    eprintln!(
        "\n{}",
        "Watching for changes... (press Ctrl+C to stop)".dimmed()
    );
}

fn analyze_and_report(config: &fallow_config::ResolvedConfig, opts: &WatchOptions<'_>) -> ExitCode {
    let start = Instant::now();
    let results = match fallow_core::analyze(config) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Analysis error: {e}");
            return ExitCode::from(2);
        }
    };
    let elapsed = start.elapsed();
    let ctx = report::ReportContext {
        root: &config.root,
        rules: &config.rules,
        elapsed,
        quiet: opts.quiet,
        explain: opts.explain,
        group_by: None,
        top: None,
        summary: false,
        show_explain_tip: true,
        baseline_matched: None,
        health_action_opts: report::HealthActionOptions::default(),
    };
    let report_code = report::print_results(&results, &ctx, config.output, None);
    if report_code != ExitCode::SUCCESS {
        eprintln!("Warning: report output failed");
    }
    ExitCode::SUCCESS
}

fn reload_config_or_keep_previous(
    config: &mut fallow_config::ResolvedConfig,
    opts: &WatchOptions<'_>,
    load: LoadConfigFn,
) {
    match load(
        opts.root,
        opts.config_path,
        opts.output,
        opts.no_cache,
        opts.threads,
        opts.production,
        opts.quiet,
    ) {
        Ok(mut reloaded) => {
            if opts.include_entry_exports {
                reloaded.include_entry_exports = true;
            }
            *config = reloaded;
        }
        Err(_) => {
            eprintln!("Warning: failed to reload config, using previous configuration");
        }
    }
}

pub fn run_watch(opts: &WatchOptions<'_>) -> ExitCode {
    use std::sync::mpsc;
    use std::time::Duration;

    let mut config = match load_config(
        opts.root,
        opts.config_path,
        opts.output,
        opts.no_cache,
        opts.threads,
        opts.production,
        opts.quiet,
    ) {
        Ok(mut c) => {
            if opts.include_entry_exports {
                c.include_entry_exports = true;
            }
            c
        }
        Err(code) => return code,
    };

    // Run initial analysis
    let initial_status = analyze_and_report(&config, opts);
    if initial_status != ExitCode::SUCCESS {
        return initial_status;
    }
    print_waiting();

    // Set up file watcher
    let (tx, rx) = mpsc::channel();
    let mut debouncer = match new_debouncer(Duration::from_millis(500), tx) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Failed to create file watcher: {e}");
            return ExitCode::from(2);
        }
    };

    if let Err(e) = debouncer
        .watcher()
        .watch(opts.root.as_ref(), notify::RecursiveMode::Recursive)
    {
        eprintln!("Failed to watch directory: {e}");
        return ExitCode::from(2);
    }

    loop {
        match rx.recv() {
            Ok(Ok(events)) => {
                let changed = collect_changed_paths(&events, opts.root);
                if changed.is_empty() {
                    continue;
                }

                if opts.clear_screen && std::io::stderr().is_terminal() {
                    eprint!("{CLEAR_SCREEN}");
                }

                // Show which files changed
                for path in &changed {
                    eprintln!("{} {path}", "Changed:".dimmed());
                }
                eprintln!();

                reload_config_or_keep_previous(&mut config, opts, load_config);

                let status = analyze_and_report(&config, opts);
                if status != ExitCode::SUCCESS {
                    eprintln!("Watch analysis failed; continuing to watch for changes");
                }
                print_waiting();
            }
            Ok(Err(e)) => {
                eprintln!("Watch error: {e:?}");
            }
            Err(e) => {
                eprintln!("Channel error: {e}");
                return ExitCode::from(2);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_config::FallowConfig;
    use notify_debouncer_mini::{DebouncedEvent, DebouncedEventKind};

    // ── is_relevant_source ───────────────────────────────────────────

    #[test]
    fn relevant_source_ts_extensions() {
        assert!(is_relevant_source(Path::new("src/index.ts")));
        assert!(is_relevant_source(Path::new("app.tsx")));
        assert!(is_relevant_source(Path::new("lib/utils.mts")));
        assert!(is_relevant_source(Path::new("lib/utils.cts")));
    }

    #[test]
    fn relevant_source_js_extensions() {
        assert!(is_relevant_source(Path::new("src/index.js")));
        assert!(is_relevant_source(Path::new("app.jsx")));
        assert!(is_relevant_source(Path::new("lib/utils.mjs")));
        assert!(is_relevant_source(Path::new("lib/utils.cjs")));
    }

    #[test]
    fn relevant_source_framework_extensions() {
        assert!(is_relevant_source(Path::new("App.vue")));
        assert!(is_relevant_source(Path::new("Page.svelte")));
        assert!(is_relevant_source(Path::new("page.astro")));
        assert!(is_relevant_source(Path::new("doc.mdx")));
    }

    #[test]
    fn relevant_source_style_extensions() {
        assert!(is_relevant_source(Path::new("styles.css")));
        assert!(is_relevant_source(Path::new("theme.scss")));
    }

    #[test]
    fn not_relevant_source() {
        assert!(!is_relevant_source(Path::new("README.md")));
        assert!(!is_relevant_source(Path::new("image.png")));
        assert!(!is_relevant_source(Path::new("data.json")));
        assert!(!is_relevant_source(Path::new("script.py")));
        assert!(!is_relevant_source(Path::new("Cargo.toml")));
        assert!(!is_relevant_source(Path::new("no_extension")));
    }

    // ── is_relevant_config ───────────────────────────────────────────

    #[test]
    fn relevant_config_files() {
        assert!(is_relevant_config(Path::new("package.json")));
        assert!(is_relevant_config(Path::new("/project/package.json")));
        assert!(is_relevant_config(Path::new(".fallowrc.json")));
        assert!(is_relevant_config(Path::new(".fallowrc.jsonc")));
        assert!(is_relevant_config(Path::new("fallow.toml")));
        assert!(is_relevant_config(Path::new(".fallow.toml")));
        assert!(is_relevant_config(Path::new("tsconfig.json")));
    }

    #[test]
    fn not_relevant_config() {
        assert!(!is_relevant_config(Path::new("eslint.config.js")));
        assert!(!is_relevant_config(Path::new("jest.config.ts")));
        assert!(!is_relevant_config(Path::new("package-lock.json")));
        assert!(!is_relevant_config(Path::new("tsconfig.build.json")));
        assert!(!is_relevant_config(Path::new("README.md")));
    }

    // ── collect_changed_paths ────────────────────────────────────────

    fn make_event(path: &str, kind: DebouncedEventKind) -> DebouncedEvent {
        DebouncedEvent {
            path: PathBuf::from(path),
            kind,
        }
    }

    #[test]
    fn collect_changed_paths_filters_non_source() {
        let root = PathBuf::from("/project");
        let events = vec![
            make_event("/project/src/index.ts", DebouncedEventKind::Any),
            make_event("/project/README.md", DebouncedEventKind::Any),
            make_event("/project/image.png", DebouncedEventKind::Any),
        ];
        let paths = collect_changed_paths(&events, &root);
        assert_eq!(paths, vec!["src/index.ts"]);
    }

    #[test]
    fn collect_changed_paths_includes_config() {
        let root = PathBuf::from("/project");
        let events = vec![
            make_event("/project/package.json", DebouncedEventKind::Any),
            make_event("/project/.fallowrc.json", DebouncedEventKind::Any),
        ];
        let paths = collect_changed_paths(&events, &root);
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&"package.json".to_string()));
        assert!(paths.contains(&".fallowrc.json".to_string()));
    }

    #[test]
    fn collect_changed_paths_deduplicates() {
        let root = PathBuf::from("/project");
        let events = vec![
            make_event("/project/src/index.ts", DebouncedEventKind::Any),
            make_event("/project/src/index.ts", DebouncedEventKind::Any),
            make_event("/project/src/index.ts", DebouncedEventKind::Any),
        ];
        let paths = collect_changed_paths(&events, &root);
        assert_eq!(paths, vec!["src/index.ts"]);
    }

    #[test]
    fn collect_changed_paths_ignores_non_any_events() {
        let root = PathBuf::from("/project");
        let events = vec![make_event(
            "/project/src/index.ts",
            DebouncedEventKind::AnyContinuous,
        )];
        let paths = collect_changed_paths(&events, &root);
        assert!(paths.is_empty());
    }

    #[test]
    fn collect_changed_paths_empty_events() {
        let root = PathBuf::from("/project");
        let paths = collect_changed_paths(&[], &root);
        assert!(paths.is_empty());
    }

    #[test]
    fn collect_changed_paths_strips_root_prefix() {
        let root = PathBuf::from("/project");
        let events = vec![make_event(
            "/project/src/deep/nested/file.tsx",
            DebouncedEventKind::Any,
        )];
        let paths = collect_changed_paths(&events, &root);
        assert_eq!(paths, vec!["src/deep/nested/file.tsx"]);
    }

    fn make_config(
        root: &Path,
        output: OutputFormat,
        threads: usize,
        quiet: bool,
    ) -> fallow_config::ResolvedConfig {
        FallowConfig {
            schema: None,
            extends: vec![],
            entry: vec![],
            ignore_patterns: vec![],
            framework: vec![],
            workspaces: None,
            ignore_dependencies: vec![],
            ignore_exports: vec![],
            ignore_catalog_references: vec![],
            ignore_dependency_overrides: vec![],
            ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
            used_class_members: vec![],
            duplicates: fallow_config::DuplicatesConfig::default(),
            health: fallow_config::HealthConfig::default(),
            rules: fallow_config::RulesConfig::default(),
            boundaries: fallow_config::BoundaryConfig::default(),
            production: false.into(),
            plugins: vec![],
            dynamically_loaded: vec![],
            overrides: vec![],
            regression: None,
            audit: fallow_config::AuditConfig::default(),
            codeowners: None,
            public_packages: vec![],
            flags: fallow_config::FlagsConfig::default(),
            resolve: fallow_config::ResolveConfig::default(),
            sealed: false,
            include_entry_exports: false,
        }
        .resolve(root.to_path_buf(), output, threads, false, quiet)
    }

    fn make_watch_options(
        root: &Path,
        output: OutputFormat,
        threads: usize,
        quiet: bool,
    ) -> WatchOptions<'_> {
        WatchOptions {
            root,
            config_path: &None,
            output,
            no_cache: false,
            threads,
            quiet,
            production: false,
            clear_screen: false,
            explain: false,
            include_entry_exports: false,
        }
    }

    #[test]
    fn reload_config_successfully_replaces_previous_config() {
        let root = Path::new("/project");
        let mut config = make_config(root, OutputFormat::Human, 1, false);
        let opts = make_watch_options(root, OutputFormat::Json, 8, true);

        reload_config_or_keep_previous(
            &mut config,
            &opts,
            |_root, _config_path, output, _no_cache, threads, _production, quiet| {
                Ok(make_config(Path::new("/project"), output, threads, quiet))
            },
        );

        assert!(matches!(config.output, OutputFormat::Json));
        assert_eq!(config.threads, 8);
        assert!(config.quiet);
    }

    #[test]
    fn reload_config_applies_include_entry_exports_override() {
        // Issue #249 follow-up: --include-entry-exports must take effect under
        // watch mode after a config reload, not just on the initial load.
        let root = Path::new("/project");
        let mut config = make_config(root, OutputFormat::Human, 1, false);
        assert!(!config.include_entry_exports);

        let mut opts = make_watch_options(root, OutputFormat::Json, 8, true);
        opts.include_entry_exports = true;

        reload_config_or_keep_previous(
            &mut config,
            &opts,
            |_root, _config_path, output, _no_cache, threads, _production, quiet| {
                Ok(make_config(Path::new("/project"), output, threads, quiet))
            },
        );

        assert!(
            config.include_entry_exports,
            "CLI flag should OR into reloaded config"
        );
    }

    #[test]
    fn reload_config_failure_keeps_previous_config() {
        let root = Path::new("/project");
        let mut config = make_config(root, OutputFormat::Human, 1, false);
        let opts = make_watch_options(root, OutputFormat::Json, 8, true);

        reload_config_or_keep_previous(
            &mut config,
            &opts,
            |_root, _config_path, _output, _no_cache, _threads, _production, _quiet| {
                Err(ExitCode::from(2))
            },
        );

        assert!(matches!(config.output, OutputFormat::Human));
        assert_eq!(config.threads, 1);
        assert!(!config.quiet);
    }
}
