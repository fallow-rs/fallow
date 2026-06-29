use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use colored::Colorize;
use fallow_config::OutputFormat;
use fallow_engine::discover;
use ignore::Match;
use notify::{RecommendedWatcher, Watcher};
use rustc_hash::FxHashSet;

use crate::report;
use crate::runtime_support::{LoadConfigArgs, load_config};

/// ANSI escape: clear screen + scrollback + move cursor home.
const CLEAR_SCREEN: &str = "\x1B[2J\x1B[3J\x1B[H";
const DEBOUNCE_WINDOW: Duration = Duration::from_millis(500);
const ROOT_POLL_INTERVAL: Duration = Duration::from_secs(1);
const REATTACH_ERROR_INTERVAL: Duration = Duration::from_secs(5);

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
    /// Mirror of the global `--include-entry-exports` flag.
    pub include_entry_exports: bool,
}

type LoadConfigFn = fn(
    root: &Path,
    config_path: &Option<PathBuf>,
    args: LoadConfigArgs,
) -> Result<fallow_config::ResolvedConfig, ExitCode>;

fn is_relevant_source(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| discover::SOURCE_EXTENSIONS.contains(&ext))
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

fn has_disallowed_hidden_dir(relative: &Path) -> bool {
    relative.parent().is_some_and(|parent| {
        parent.components().any(|component| {
            let name = component.as_os_str();
            name.to_string_lossy().starts_with('.') && !discover::is_allowed_hidden_dir(name)
        })
    })
}

fn build_production_glob_set() -> Option<globset::GlobSet> {
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in discover::PRODUCTION_EXCLUDE_PATTERNS {
        if let Ok(glob) = globset::GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
        {
            builder.add(glob);
        }
    }
    builder.build().ok()
}

#[derive(Clone)]
struct WatchFilter {
    root: PathBuf,
    ignore_patterns: globset::GlobSet,
    production_excludes: Option<globset::GlobSet>,
    gitignores: Vec<ignore::gitignore::Gitignore>,
    global_gitignore: ignore::gitignore::Gitignore,
}

impl WatchFilter {
    fn new(config: &fallow_config::ResolvedConfig) -> Self {
        let gitignores = build_project_gitignores(config);
        let (global_gitignore, _) = ignore::gitignore::Gitignore::global();
        Self {
            root: config.root.clone(),
            ignore_patterns: config.ignore_patterns.clone(),
            production_excludes: config.production.then(build_production_glob_set).flatten(),
            gitignores,
            global_gitignore,
        }
    }

    fn allows(&self, path: &Path) -> bool {
        if !path.starts_with(&self.root) {
            return false;
        }
        let relative = path.strip_prefix(&self.root).unwrap_or(path);
        if has_disallowed_hidden_dir(relative) {
            return false;
        }
        if self.ignore_patterns.is_match(relative) {
            return false;
        }
        if self
            .production_excludes
            .as_ref()
            .is_some_and(|excludes| excludes.is_match(relative))
        {
            return false;
        }
        let is_dir = path.is_dir();
        match self.project_gitignore_match(path, is_dir) {
            Some(true) => return false,
            Some(false) => {}
            None => {
                if matches!(
                    self.global_gitignore.matched(path, is_dir),
                    Match::Ignore(_)
                ) {
                    return false;
                }
            }
        }
        is_relevant_source(path) || is_relevant_config(path) || path == self.root.join(".gitignore")
    }

    fn project_gitignore_match(&self, path: &Path, is_dir: bool) -> Option<bool> {
        let mut ignored = None;
        for gitignore in &self.gitignores {
            match gitignore.matched_path_or_any_parents(path, is_dir) {
                Match::Ignore(_) => ignored = Some(true),
                Match::Whitelist(_) => ignored = Some(false),
                Match::None => {}
            }
        }
        ignored
    }
}

fn build_project_gitignores(
    config: &fallow_config::ResolvedConfig,
) -> Vec<ignore::gitignore::Gitignore> {
    let root = &config.root;
    let mut gitignores = Vec::new();

    let git_exclude = root.join(".git/info/exclude");
    if let Some(gitignore) = build_gitignore(root, &git_exclude) {
        gitignores.push(gitignore);
    }

    for path in discover_project_gitignores(root, &config.ignore_patterns) {
        if let Some(base) = path.parent()
            && let Some(gitignore) = build_gitignore(base, &path)
        {
            gitignores.push(gitignore);
        }
    }

    gitignores
}

fn build_gitignore(base: &Path, path: &Path) -> Option<ignore::gitignore::Gitignore> {
    let mut builder = ignore::gitignore::GitignoreBuilder::new(base);
    let _ = builder.add(path);
    builder.build().ok()
}

fn discover_project_gitignores(root: &Path, ignore_patterns: &globset::GlobSet) -> Vec<PathBuf> {
    let root = root.to_path_buf();
    let ignore_patterns = ignore_patterns.clone();
    let filter_root = root.clone();
    let mut walk_builder = ignore::WalkBuilder::new(&root);
    walk_builder
        .hidden(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .filter_entry(move |entry| {
            let relative = entry
                .path()
                .strip_prefix(&filter_root)
                .unwrap_or_else(|_| entry.path());
            !has_disallowed_hidden_dir(relative) && !ignore_patterns.is_match(relative)
        });

    let mut paths = Vec::new();
    for entry in walk_builder.build().flatten() {
        if entry
            .file_type()
            .is_some_and(|file_type| !file_type.is_dir())
            && entry.file_name() == ".gitignore"
        {
            paths.push(entry.into_path());
        }
    }
    paths.sort_unstable();
    paths
}

fn filter_event_paths(event: notify::Event, filter: &WatchFilter) -> Vec<PathBuf> {
    let mut seen = FxHashSet::default();
    let mut paths = Vec::new();
    for path in event.paths {
        if !filter.allows(&path) {
            continue;
        }
        if seen.insert(path.clone()) {
            paths.push(path);
        }
    }
    paths
}

#[derive(Debug, Default)]
struct PathDebouncer {
    paths: Vec<PathBuf>,
    seen: FxHashSet<PathBuf>,
    last_update: Option<Instant>,
}

impl PathDebouncer {
    fn push_paths(&mut self, paths: Vec<PathBuf>, now: Instant) {
        if paths.is_empty() {
            return;
        }
        for path in paths {
            if self.seen.insert(path.clone()) {
                self.paths.push(path);
            }
        }
        self.last_update = Some(now);
    }

    fn drain_ready(&mut self, now: Instant, timeout: Duration) -> Option<Vec<PathBuf>> {
        if self
            .last_update
            .is_some_and(|updated| now.duration_since(updated) >= timeout)
        {
            self.last_update = None;
            self.seen.clear();
            Some(std::mem::take(&mut self.paths))
        } else {
            None
        }
    }

    fn clear(&mut self) {
        self.paths.clear();
        self.seen.clear();
        self.last_update = None;
    }
}

fn display_changed_paths(paths: Vec<PathBuf>, root: &Path) -> Vec<String> {
    let mut seen = FxHashSet::default();
    let mut display_paths = Vec::with_capacity(paths.len());
    for path in paths {
        let display = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .display()
            .to_string();
        if seen.insert(display.clone()) {
            display_paths.push(display);
        }
    }
    display_paths
}

fn print_waiting(opts: &WatchOptions<'_>) {
    if opts.quiet {
        return;
    }
    eprintln!(
        "\n{}",
        "Watching for changes... (press Ctrl+C to stop)".dimmed()
    );
}

fn analyze_and_report(config: &fallow_config::ResolvedConfig, opts: &WatchOptions<'_>) -> ExitCode {
    let start = Instant::now();
    let results = match fallow_engine::analyze(config) {
        Ok(analysis) => analysis.results,
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
        summary_heading: true,
        show_explain_tip: true,
        baseline_matched: None,
        config_fixable: crate::fix::is_config_fixable(&config.root, opts.config_path.as_ref()),
        skip_score_and_trend: false,
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
        LoadConfigArgs {
            output: opts.output,
            no_cache: opts.no_cache,
            threads: opts.threads,
            production: opts.production,
            quiet: opts.quiet,
        },
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
    let _ = crate::signal::install_handlers();
    let _graceful = crate::signal::GracefulModeGuard::new();

    let config = match load_watch_config(opts) {
        Ok(config) => config,
        Err(code) => return code,
    };
    if let Err(code) = run_initial_watch_analysis(&config, opts) {
        return code;
    }

    let mut state = match WatchLoopState::new(opts, config) {
        Ok(state) => state,
        Err(code) => return code,
    };
    run_watch_loop(opts, &mut state)
}

fn run_initial_watch_analysis(
    config: &fallow_config::ResolvedConfig,
    opts: &WatchOptions<'_>,
) -> Result<(), ExitCode> {
    let initial_status = analyze_and_report(config, opts);
    if initial_status != ExitCode::SUCCESS {
        return Err(initial_status);
    }
    print_waiting(opts);
    Ok(())
}

struct WatchLoopState {
    config: fallow_config::ResolvedConfig,
    filter: Arc<Mutex<WatchFilter>>,
    watcher: Option<RecommendedWatcher>,
    tx: mpsc::Sender<WatchEvent>,
    rx: mpsc::Receiver<WatchEvent>,
    debouncer: PathDebouncer,
    detached: bool,
    next_root_check: Instant,
    last_reattach_error: Option<Instant>,
}

impl WatchLoopState {
    fn new(
        opts: &WatchOptions<'_>,
        config: fallow_config::ResolvedConfig,
    ) -> Result<Self, ExitCode> {
        let (tx, rx) = mpsc::channel();
        let filter = Arc::new(Mutex::new(WatchFilter::new(&config)));
        let watcher = match create_watcher(opts.root, Arc::clone(&filter), tx.clone()) {
            Ok(watcher) => Some(watcher),
            Err(e) => {
                eprintln!("Failed to create file watcher: {e}");
                return Err(ExitCode::from(2));
            }
        };

        Ok(Self {
            config,
            filter,
            watcher,
            tx,
            rx,
            debouncer: PathDebouncer::default(),
            detached: false,
            next_root_check: Instant::now() + ROOT_POLL_INTERVAL,
            last_reattach_error: None,
        })
    }
}

fn run_watch_loop(opts: &WatchOptions<'_>, state: &mut WatchLoopState) -> ExitCode {
    loop {
        if crate::signal::is_shutting_down() {
            eprintln!("Watch stopped.");
            return ExitCode::SUCCESS;
        }

        let now = Instant::now();
        if now >= state.next_root_check {
            state.next_root_check = now + ROOT_POLL_INTERVAL;
            handle_root_lifecycle(
                opts,
                RootLifecycleState {
                    config: &mut state.config,
                    filter: &state.filter,
                    watcher: &mut state.watcher,
                    tx: &state.tx,
                    debouncer: &mut state.debouncer,
                    detached: &mut state.detached,
                    last_reattach_error: &mut state.last_reattach_error,
                },
            );
        }

        match receive_watch_event(&state.rx, &mut state.debouncer, state.detached) {
            WatchPoll::Continue => continue,
            WatchPoll::Disconnected => {
                eprintln!("Channel error: notify sender disconnected");
                return ExitCode::from(2);
            }
            WatchPoll::Idle => {}
        }

        if !state.detached {
            run_ready_reanalysis(&mut state.config, opts, &mut state.debouncer);
        }
    }
}

/// Outcome of polling the watch channel for one debounce window.
enum WatchPoll {
    /// Skip the rest of this loop iteration (event arrived while detached).
    Continue,
    /// The notify sender hung up; the caller should exit.
    Disconnected,
    /// Nothing actionable; fall through to the debounce drain.
    Idle,
}

/// Poll the watch channel for up to 200ms, pushing any allowed paths into the
/// debouncer. Mirrors the original inline recv handling.
fn receive_watch_event(
    rx: &std::sync::mpsc::Receiver<WatchEvent>,
    debouncer: &mut PathDebouncer,
    detached: bool,
) -> WatchPoll {
    use std::sync::mpsc::RecvTimeoutError;

    match rx.recv_timeout(Duration::from_millis(200)) {
        Ok(Ok(paths)) => {
            if detached {
                return WatchPoll::Continue;
            }
            debouncer.push_paths(paths, Instant::now());
            WatchPoll::Idle
        }
        Ok(Err(e)) => {
            eprintln!("Watch error: {e:?}");
            WatchPoll::Idle
        }
        Err(RecvTimeoutError::Timeout) => WatchPoll::Idle,
        Err(RecvTimeoutError::Disconnected) => WatchPoll::Disconnected,
    }
}

/// Load the watch config, applying the `--include-entry-exports` override.
fn load_watch_config(opts: &WatchOptions<'_>) -> Result<fallow_config::ResolvedConfig, ExitCode> {
    let mut config = load_config(
        opts.root,
        opts.config_path,
        LoadConfigArgs {
            output: opts.output,
            no_cache: opts.no_cache,
            threads: opts.threads,
            production: opts.production,
            quiet: opts.quiet,
        },
    )?;
    if opts.include_entry_exports {
        config.include_entry_exports = true;
    }
    Ok(config)
}

/// Drain the debouncer; if a non-empty batch is ready, reload config and
/// re-run the analysis. No-op when no batch is ready or all paths dedupe away.
fn run_ready_reanalysis(
    config: &mut fallow_config::ResolvedConfig,
    opts: &WatchOptions<'_>,
    debouncer: &mut PathDebouncer,
) {
    let Some(paths) = debouncer.drain_ready(Instant::now(), DEBOUNCE_WINDOW) else {
        return;
    };
    let changed = display_changed_paths(paths, opts.root);
    if changed.is_empty() {
        return;
    }

    if opts.clear_screen && std::io::stderr().is_terminal() {
        eprint!("{CLEAR_SCREEN}");
    }

    for path in &changed {
        eprintln!("{} {path}", "Changed:".dimmed());
    }
    eprintln!();

    reload_config_or_keep_previous(config, opts, load_config);

    let status = analyze_and_report(config, opts);
    if status != ExitCode::SUCCESS {
        eprintln!("Watch analysis failed; continuing to watch for changes");
    }
    print_waiting(opts);
}

type WatchEvent = Result<Vec<PathBuf>, notify::Error>;

fn create_watcher(
    root: &Path,
    filter: Arc<Mutex<WatchFilter>>,
    tx: std::sync::mpsc::Sender<WatchEvent>,
) -> notify::Result<RecommendedWatcher> {
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let event = match event {
            Ok(event) => event,
            Err(err) => {
                let _ = tx.send(Err(err));
                return;
            }
        };
        let Ok(filter) = filter.lock() else {
            return;
        };
        let paths = filter_event_paths(event, &filter);
        if !paths.is_empty() {
            let _ = tx.send(Ok(paths));
        }
    })?;
    watcher.watch(root, notify::RecursiveMode::Recursive)?;
    Ok(watcher)
}

fn replace_watch_filter(filter: &Arc<Mutex<WatchFilter>>, config: &fallow_config::ResolvedConfig) {
    if let Ok(mut guard) = filter.lock() {
        *guard = WatchFilter::new(config);
    }
}

struct RootLifecycleState<'a> {
    config: &'a mut fallow_config::ResolvedConfig,
    filter: &'a Arc<Mutex<WatchFilter>>,
    watcher: &'a mut Option<RecommendedWatcher>,
    tx: &'a std::sync::mpsc::Sender<WatchEvent>,
    debouncer: &'a mut PathDebouncer,
    detached: &'a mut bool,
    last_reattach_error: &'a mut Option<Instant>,
}

fn handle_root_lifecycle(opts: &WatchOptions<'_>, state: RootLifecycleState<'_>) {
    let RootLifecycleState {
        config,
        filter,
        watcher,
        tx,
        debouncer,
        detached,
        last_reattach_error,
    } = state;

    let root_exists = opts.root.metadata().is_ok();
    if !root_exists {
        if !*detached {
            watcher.take();
            debouncer.clear();
            *detached = true;
            *last_reattach_error = None;
            if !opts.quiet {
                eprintln!("Watch root disappeared; waiting for it to reappear...");
            }
        }
        return;
    }

    if !*detached {
        return;
    }

    reload_config_or_keep_previous(config, opts, load_config);
    replace_watch_filter(filter, config);
    match create_watcher(opts.root, Arc::clone(filter), tx.clone()) {
        Ok(new_watcher) => {
            *watcher = Some(new_watcher);
            debouncer.clear();
            *detached = false;
            *last_reattach_error = None;
            if !opts.quiet {
                eprintln!("Watch root re-attached; running analysis...");
            }
            let status = analyze_and_report(config, opts);
            if status != ExitCode::SUCCESS {
                eprintln!("Watch analysis failed; continuing to watch for changes");
            }
            print_waiting(opts);
        }
        Err(e) => {
            let now = Instant::now();
            if last_reattach_error
                .is_none_or(|last| now.duration_since(last) >= REATTACH_ERROR_INTERVAL)
            {
                if !opts.quiet {
                    eprintln!("Failed to re-attach watch root: {e}");
                }
                *last_reattach_error = Some(now);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_config::FallowConfig;
    use notify::event::EventKind;

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

    #[test]
    fn disallowed_hidden_dirs_match_discovery_filter() {
        assert!(has_disallowed_hidden_dir(Path::new(".fallow/.gitignore")));
        assert!(has_disallowed_hidden_dir(Path::new(".cache/file.ts")));
        assert!(!has_disallowed_hidden_dir(Path::new(".storybook/main.ts")));
        assert!(!has_disallowed_hidden_dir(Path::new("src/.generated.ts")));
    }

    fn make_event(paths: &[&Path]) -> notify::Event {
        let mut event = notify::Event::new(EventKind::Any);
        for path in paths {
            event = event.add_path((*path).to_path_buf());
        }
        event
    }

    #[test]
    fn watch_filter_rejects_non_source() {
        let root = PathBuf::from("/project");
        let config = make_config(&root, OutputFormat::Human, 1, false);
        let filter = WatchFilter::new(&config);
        let event = make_event(&[
            Path::new("/project/src/index.ts"),
            Path::new("/project/README.md"),
            Path::new("/project/image.png"),
        ]);
        let paths = filter_event_paths(event, &filter);
        assert_eq!(display_changed_paths(paths, &root), vec!["src/index.ts"]);
    }

    #[test]
    fn watch_filter_includes_config() {
        let root = PathBuf::from("/project");
        let config = make_config(&root, OutputFormat::Human, 1, false);
        let filter = WatchFilter::new(&config);
        let event = make_event(&[
            Path::new("/project/package.json"),
            Path::new("/project/.fallowrc.json"),
        ]);
        let paths = display_changed_paths(filter_event_paths(event, &filter), &root);
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&"package.json".to_string()));
        assert!(paths.contains(&".fallowrc.json".to_string()));
    }

    #[test]
    fn watch_filter_deduplicates() {
        let root = PathBuf::from("/project");
        let config = make_config(&root, OutputFormat::Human, 1, false);
        let filter = WatchFilter::new(&config);
        let event = make_event(&[
            Path::new("/project/src/index.ts"),
            Path::new("/project/src/index.ts"),
            Path::new("/project/src/index.ts"),
        ]);
        let paths = display_changed_paths(filter_event_paths(event, &filter), &root);
        assert_eq!(paths, vec!["src/index.ts"]);
    }

    #[test]
    fn watch_filter_rejects_default_ignored_paths() {
        let root = PathBuf::from("/project");
        let config = make_config(&root, OutputFormat::Human, 1, false);
        let filter = WatchFilter::new(&config);
        let event = make_event(&[
            Path::new("/project/node_modules/foo/index.ts"),
            Path::new("/project/dist/index.ts"),
            Path::new("/project/.git/config"),
            Path::new("/project/build/index.ts"),
            Path::new("/project/src/vendor.min.js"),
        ]);
        let paths = filter_event_paths(event, &filter);
        assert!(paths.is_empty());
    }

    #[test]
    fn watch_filter_allows_root_gitignore_but_rejects_internal_gitignore() {
        let root = PathBuf::from("/project");
        let config = make_config(&root, OutputFormat::Human, 1, false);
        let filter = WatchFilter::new(&config);
        let event = make_event(&[
            Path::new("/project/.gitignore"),
            Path::new("/project/.fallow/.gitignore"),
        ]);
        let paths = display_changed_paths(filter_event_paths(event, &filter), &root);
        assert_eq!(paths, vec![".gitignore"]);
    }

    #[test]
    fn watch_filter_rejects_user_ignore_patterns() {
        let root = PathBuf::from("/project");
        let config = make_config_with_ignores(
            &root,
            OutputFormat::Human,
            1,
            false,
            vec!["src/generated/**".to_string()],
        );
        let filter = WatchFilter::new(&config);
        let event = make_event(&[Path::new("/project/src/generated/client.ts")]);
        let paths = filter_event_paths(event, &filter);
        assert!(paths.is_empty());
    }

    #[test]
    fn watch_filter_rejects_gitignored_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(".gitignore"), "ignored/**\n").expect("write gitignore");
        let config = make_config(dir.path(), OutputFormat::Human, 1, false);
        let filter = WatchFilter::new(&config);
        let event = make_event(&[&dir.path().join("ignored/file.ts")]);
        let paths = filter_event_paths(event, &filter);
        assert!(paths.is_empty());
    }

    #[test]
    fn watch_filter_rejects_nested_gitignored_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("packages/web")).expect("create package dir");
        std::fs::write(dir.path().join("packages/web/.gitignore"), "generated/**\n")
            .expect("write nested gitignore");
        let config = make_config(dir.path(), OutputFormat::Human, 1, false);
        let filter = WatchFilter::new(&config);
        let event = make_event(&[&dir.path().join("packages/web/generated/client.ts")]);
        let paths = filter_event_paths(event, &filter);
        assert!(paths.is_empty());
    }

    #[test]
    fn watch_filter_project_whitelist_overrides_parent_ignore() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("packages/web/generated"))
            .expect("create package dir");
        std::fs::write(dir.path().join(".gitignore"), "packages/web/generated/**\n")
            .expect("write root gitignore");
        std::fs::write(
            dir.path().join("packages/web/.gitignore"),
            "!generated/client.ts\n",
        )
        .expect("write nested gitignore");
        let config = make_config(dir.path(), OutputFormat::Human, 1, false);
        let filter = WatchFilter::new(&config);
        let event = make_event(&[&dir.path().join("packages/web/generated/client.ts")]);
        let paths = display_changed_paths(filter_event_paths(event, &filter), dir.path());
        assert_eq!(paths, vec!["packages/web/generated/client.ts"]);
    }

    #[test]
    fn watch_filter_rejects_ignored_config_files() {
        let root = PathBuf::from("/project");
        let config = make_config_with_ignores(
            &root,
            OutputFormat::Human,
            1,
            false,
            vec!["package.json".to_string()],
        );
        let filter = WatchFilter::new(&config);
        let event = make_event(&[Path::new("/project/package.json")]);
        let paths = filter_event_paths(event, &filter);
        assert!(paths.is_empty());
    }

    #[test]
    fn display_changed_paths_strips_root_prefix() {
        let root = PathBuf::from("/project");
        let paths = display_changed_paths(
            vec![PathBuf::from("/project/src/deep/nested/file.tsx")],
            &root,
        );
        assert_eq!(paths, vec!["src/deep/nested/file.tsx"]);
    }

    #[test]
    fn path_debouncer_emits_one_deduplicated_batch_after_quiet_window() {
        let start = Instant::now();
        let mut debouncer = PathDebouncer::default();
        debouncer.push_paths(
            vec![
                PathBuf::from("/project/src/index.ts"),
                PathBuf::from("/project/src/index.ts"),
            ],
            start,
        );
        assert!(
            debouncer
                .drain_ready(start + Duration::from_millis(499), DEBOUNCE_WINDOW)
                .is_none()
        );

        let paths = debouncer
            .drain_ready(start + DEBOUNCE_WINDOW, DEBOUNCE_WINDOW)
            .expect("ready batch");
        assert_eq!(paths, vec![PathBuf::from("/project/src/index.ts")]);
    }

    #[test]
    fn empty_or_ignored_batches_do_not_extend_debounce_window() {
        let start = Instant::now();
        let mut debouncer = PathDebouncer::default();
        debouncer.push_paths(vec![PathBuf::from("/project/src/index.ts")], start);
        debouncer.push_paths(Vec::new(), start + Duration::from_millis(400));

        assert!(
            debouncer
                .drain_ready(start + DEBOUNCE_WINDOW, DEBOUNCE_WINDOW)
                .is_some()
        );
    }

    #[test]
    fn root_lifecycle_detaches_and_reattaches() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("project");
        std::fs::create_dir(&root).expect("create root");
        let mut config = make_config(&root, OutputFormat::Human, 1, true);
        let opts = make_watch_options(&root, OutputFormat::Human, 1, true);
        let filter = Arc::new(Mutex::new(WatchFilter::new(&config)));
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut watcher = None;
        let mut debouncer = PathDebouncer::default();
        let mut detached = false;
        let mut last_reattach_error = None;

        std::fs::remove_dir(&root).expect("remove root");
        handle_root_lifecycle(
            &opts,
            RootLifecycleState {
                config: &mut config,
                filter: &filter,
                watcher: &mut watcher,
                tx: &tx,
                debouncer: &mut debouncer,
                detached: &mut detached,
                last_reattach_error: &mut last_reattach_error,
            },
        );
        assert!(detached);
        assert!(watcher.is_none());

        std::fs::create_dir(&root).expect("recreate root");
        handle_root_lifecycle(
            &opts,
            RootLifecycleState {
                config: &mut config,
                filter: &filter,
                watcher: &mut watcher,
                tx: &tx,
                debouncer: &mut debouncer,
                detached: &mut detached,
                last_reattach_error: &mut last_reattach_error,
            },
        );
        assert!(!detached);
        assert!(watcher.is_some());
    }

    fn make_config(
        root: &Path,
        output: OutputFormat,
        threads: usize,
        quiet: bool,
    ) -> fallow_config::ResolvedConfig {
        make_config_with_ignores(root, output, threads, quiet, Vec::new())
    }

    fn make_config_with_ignores(
        root: &Path,
        output: OutputFormat,
        threads: usize,
        quiet: bool,
        ignore_patterns: Vec<String>,
    ) -> fallow_config::ResolvedConfig {
        FallowConfig {
            schema: None,
            extends: vec![],
            entry: vec![],
            ignore_patterns,
            framework: vec![],
            workspaces: None,
            ignore_dependencies: vec![],
            ignore_unresolved_imports: vec![],
            ignore_exports: vec![],
            ignore_catalog_references: vec![],
            ignore_dependency_overrides: vec![],
            ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
            used_class_members: vec![],
            ignore_decorators: vec![],
            unused_component_props: fallow_config::UnusedComponentPropsConfig::default(),
            duplicates: fallow_config::DuplicatesConfig::default(),
            health: fallow_config::HealthConfig::default(),
            rules: fallow_config::RulesConfig::default(),
            boundaries: fallow_config::BoundaryConfig::default(),
            production: false.into(),
            plugins: vec![],
            rule_packs: vec![],
            dynamically_loaded: vec![],
            overrides: vec![],
            regression: None,
            audit: fallow_config::AuditConfig::default(),
            codeowners: None,
            public_packages: vec![],
            flags: fallow_config::FlagsConfig::default(),
            security: fallow_config::SecurityConfig::default(),
            fix: fallow_config::FixConfig::default(),
            resolve: fallow_config::ResolveConfig::default(),
            sealed: false,
            include_entry_exports: false,
            auto_imports: false,
            cache: fallow_config::CacheConfig::default(),
        }
        .resolve(root.to_path_buf(), output, threads, false, quiet, None)
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

        reload_config_or_keep_previous(&mut config, &opts, |_root, _config_path, args| {
            Ok(make_config(
                Path::new("/project"),
                args.output,
                args.threads,
                args.quiet,
            ))
        });

        assert!(matches!(config.output, OutputFormat::Json));
        assert_eq!(config.threads, 8);
        assert!(config.quiet);
    }

    #[test]
    fn reload_config_applies_include_entry_exports_override() {
        let root = Path::new("/project");
        let mut config = make_config(root, OutputFormat::Human, 1, false);
        assert!(!config.include_entry_exports);

        let mut opts = make_watch_options(root, OutputFormat::Json, 8, true);
        opts.include_entry_exports = true;

        reload_config_or_keep_previous(&mut config, &opts, |_root, _config_path, args| {
            Ok(make_config(
                Path::new("/project"),
                args.output,
                args.threads,
                args.quiet,
            ))
        });

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

        reload_config_or_keep_previous(&mut config, &opts, |_root, _config_path, _args| {
            Err(ExitCode::from(2))
        });

        assert!(matches!(config.output, OutputFormat::Human));
        assert_eq!(config.threads, 1);
        assert!(!config.quiet);
    }
}
