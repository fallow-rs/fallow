use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use fallow_config::OutputFormat;

const DO_NOT_TRACK_ENV: &str = "DO_NOT_TRACK";
const TELEMETRY_DISABLED_ENV: &str = "FALLOW_TELEMETRY_DISABLED";
const UPDATE_CHECK_ENV: &str = "FALLOW_UPDATE_CHECK";

static CACHE_NOTICE: LazyLock<Mutex<Option<CacheNoticeCandidate>>> =
    LazyLock::new(|| Mutex::new(None));

#[derive(Clone, Debug)]
struct CacheNoticeCandidate {
    root: PathBuf,
    cache_dir: PathBuf,
    existed_before: bool,
}

#[derive(Clone, Copy, Debug)]
struct CacheNoticeContext {
    output: OutputFormat,
    quiet: bool,
    stdout_tty: bool,
    stderr_tty: bool,
    env_disabled: bool,
}

pub fn record_candidate(
    root: &Path,
    cache_dir: &Path,
    output: OutputFormat,
    quiet: bool,
    no_cache: bool,
) {
    if no_cache {
        return;
    }
    let context = CacheNoticeContext {
        output,
        quiet,
        stdout_tty: std::io::stdout().is_terminal(),
        stderr_tty: std::io::stderr().is_terminal(),
        env_disabled: env_disabled(),
    };
    if !should_consider_notice(context) {
        return;
    }
    let Ok(mut slot) = CACHE_NOTICE.lock() else {
        return;
    };
    if slot.is_some() {
        return;
    }
    *slot = Some(CacheNoticeCandidate {
        root: root.to_path_buf(),
        cache_dir: cache_dir.to_path_buf(),
        existed_before: cache_dir.exists(),
    });
}

pub fn maybe_print_created_notice() -> bool {
    let candidate = CACHE_NOTICE.lock().ok().and_then(|mut slot| slot.take());
    let Some(candidate) = candidate else {
        return false;
    };
    if candidate.existed_before || !candidate.cache_dir.exists() {
        return false;
    }
    eprintln!(
        "note: caching analysis to {} (set FALLOW_CACHE_DIR or cache.dir to relocate, --no-cache to disable)",
        display_cache_dir(&candidate.root, &candidate.cache_dir)
    );
    true
}

fn should_consider_notice(context: CacheNoticeContext) -> bool {
    matches!(context.output, OutputFormat::Human)
        && !context.quiet
        && context.stdout_tty
        && context.stderr_tty
        && !context.env_disabled
}

fn display_cache_dir(root: &Path, cache_dir: &Path) -> String {
    let path = cache_dir
        .strip_prefix(root)
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty())
        .unwrap_or(cache_dir);
    let mut display = path.to_string_lossy().replace('\\', "/");
    if !display.ends_with('/') {
        display.push('/');
    }
    display
}

fn env_disabled() -> bool {
    env_truthy(DO_NOT_TRACK_ENV)
        || env_truthy(TELEMETRY_DISABLED_ENV)
        || update_check_off()
        || is_ci()
}

fn update_check_off() -> bool {
    std::env::var(UPDATE_CHECK_ENV).ok().is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "off" | "0" | "false" | "disabled" | "no"
        )
    })
}

fn is_ci() -> bool {
    std::env::var_os("CI").is_some()
        || std::env::var_os("GITHUB_ACTIONS").is_some()
        || std::env::var_os("GITLAB_CI").is_some()
}

fn env_truthy(name: &str) -> bool {
    std::env::var(name).ok().is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn clear_candidate() {
        *CACHE_NOTICE.lock().expect("cache notice lock") = None;
    }

    #[test]
    fn notice_requires_human_non_quiet_tty_and_enabled_env() {
        let active = CacheNoticeContext {
            output: OutputFormat::Human,
            quiet: false,
            stdout_tty: true,
            stderr_tty: true,
            env_disabled: false,
        };
        assert!(should_consider_notice(active));

        assert!(!should_consider_notice(CacheNoticeContext {
            quiet: true,
            ..active
        }));
        assert!(!should_consider_notice(CacheNoticeContext {
            output: OutputFormat::Json,
            ..active
        }));
        assert!(!should_consider_notice(CacheNoticeContext {
            stdout_tty: false,
            ..active
        }));
        assert!(!should_consider_notice(CacheNoticeContext {
            stderr_tty: false,
            ..active
        }));
        assert!(!should_consider_notice(CacheNoticeContext {
            env_disabled: true,
            ..active
        }));
    }

    #[test]
    fn display_cache_dir_prefers_project_relative_path() {
        let root = Path::new("/repo");
        assert_eq!(
            display_cache_dir(root, Path::new("/repo/.fallow")),
            ".fallow/"
        );
        assert_eq!(
            display_cache_dir(root, Path::new("/tmp/fallow-cache")),
            "/tmp/fallow-cache/"
        );
    }

    #[test]
    fn maybe_print_created_notice_returns_false_without_candidate() {
        let _guard = TEST_LOCK.lock().expect("test lock");
        clear_candidate();

        assert!(!maybe_print_created_notice());
    }

    #[test]
    fn maybe_print_created_notice_returns_false_for_existing_cache() {
        let _guard = TEST_LOCK.lock().expect("test lock");
        clear_candidate();

        let dir = tempfile::tempdir().expect("tempdir");
        let cache_dir = dir.path().join(".fallow");
        std::fs::create_dir_all(&cache_dir).expect("create cache dir");
        *CACHE_NOTICE.lock().expect("cache notice lock") = Some(CacheNoticeCandidate {
            root: dir.path().to_path_buf(),
            cache_dir,
            existed_before: true,
        });

        assert!(!maybe_print_created_notice());
        clear_candidate();
    }

    #[test]
    fn maybe_print_created_notice_returns_false_when_cache_was_not_created() {
        let _guard = TEST_LOCK.lock().expect("test lock");
        clear_candidate();

        let dir = tempfile::tempdir().expect("tempdir");
        *CACHE_NOTICE.lock().expect("cache notice lock") = Some(CacheNoticeCandidate {
            root: dir.path().to_path_buf(),
            cache_dir: dir.path().join(".fallow"),
            existed_before: false,
        });

        assert!(!maybe_print_created_notice());
        clear_candidate();
    }

    #[test]
    fn maybe_print_created_notice_reports_new_cache_once() {
        let _guard = TEST_LOCK.lock().expect("test lock");
        clear_candidate();

        let dir = tempfile::tempdir().expect("tempdir");
        let cache_dir = dir.path().join(".fallow");
        std::fs::create_dir_all(&cache_dir).expect("create cache dir");
        *CACHE_NOTICE.lock().expect("cache notice lock") = Some(CacheNoticeCandidate {
            root: dir.path().to_path_buf(),
            cache_dir,
            existed_before: false,
        });

        assert!(maybe_print_created_notice());
        assert!(!maybe_print_created_notice());
        clear_candidate();
    }
}
