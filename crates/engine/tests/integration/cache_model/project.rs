//! A project on disk with an injected clock, and the warm and cold runs of
//! the cached analyses over it.

use std::fs::{FileTimes, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use fallow_config::{DuplicatesConfig, FallowConfig, OutputFormat, ResolvedConfig};

/// The first modification time that the clock gives. It is far in the past,
/// so every timestamp looks settled to the caches. That is the hard case: a
/// cache that trusts settled metadata must still see each content change.
const CLOCK_START_SECS: u64 = 1_600_000_000;

/// The analysis options that a step can change.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    pub production: bool,
    pub ignore_patterns: Vec<String>,
    pub conditions: Vec<String>,
}

/// The results of one run of each cached analysis, as JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    pub analysis: serde_json::Value,
    pub dupes: serde_json::Value,
}

/// A project in a temporary directory.
///
/// The layout is `<scope>/project` for the analyzed root, `<scope>/cache` for
/// the cache of the warm runs and `<scope>/cold-cache` for the cache-free
/// runs. Each write sets the modification time from the clock, so no test
/// sleeps to get distinct timestamps.
pub struct Project {
    temp: tempfile::TempDir,
    pub root: PathBuf,
    pub cache_dir: PathBuf,
    clock: SystemTime,
    pub options: Options,
}

impl Project {
    pub fn new(files: &[(&str, &str)]) -> Self {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        let cache_dir = temp.path().join("cache");
        std::fs::create_dir_all(&root).expect("create project root");
        let mut project = Self {
            temp,
            root,
            cache_dir,
            clock: SystemTime::UNIX_EPOCH + Duration::from_secs(CLOCK_START_SECS),
            options: Options::default(),
        };
        for (relative, content) in files {
            project.write(relative, content);
        }
        project
    }

    /// The temporary directory that holds the project and its caches.
    pub fn scope(&self) -> &Path {
        self.temp.path()
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    pub fn read(&self, relative: &str) -> Option<String> {
        std::fs::read_to_string(self.path(relative)).ok()
    }

    pub fn exists(&self, relative: &str) -> bool {
        self.path(relative).exists()
    }

    fn tick(&mut self) -> SystemTime {
        self.clock += Duration::from_secs(1);
        self.clock
    }

    /// Write `content` and give the file the next clock time.
    pub fn write(&mut self, relative: &str, content: &str) {
        let path = self.path(relative);
        write_file(&path, content);
        let time = self.tick();
        set_times(&path, time);
    }

    /// Write `content` and restore the previous modification time. When the
    /// length is the same too, the metadata cannot show the change.
    pub fn write_keep_mtime(&self, relative: &str, content: &str) {
        let path = self.path(relative);
        let modified = std::fs::metadata(&path)
            .and_then(|metadata| metadata.modified())
            .expect("modification time of an existing file");
        write_file(&path, content);
        set_times(&path, modified);
    }

    /// Give the file the next clock time, with no change to its content.
    pub fn touch(&mut self, relative: &str) {
        let time = self.tick();
        set_times(&self.path(relative), time);
    }

    pub fn remove(&self, relative: &str) {
        std::fs::remove_file(self.path(relative)).expect("remove project file");
    }

    pub fn rename(&self, from: &str, to: &str) {
        let target = self.path(to);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).expect("create rename target dir");
        }
        std::fs::rename(self.path(from), target).expect("rename project file");
    }

    /// The resolved configuration for a warm run (`no_cache == false`) or a
    /// cache-free run.
    pub fn config(&self, no_cache: bool) -> ResolvedConfig {
        let raw = FallowConfig {
            production: self.options.production.into(),
            ignore_patterns: self.options.ignore_patterns.clone(),
            duplicates: DuplicatesConfig {
                min_tokens: 20,
                min_lines: 3,
                // The token cache turns on only above a corpus size. A
                // template is small, so the threshold is zero to put the
                // dupes cache under test.
                min_corpus_size_for_token_cache: 0,
                ..DuplicatesConfig::default()
            },
            ..FallowConfig::default()
        };
        let mut config = raw.resolve(
            self.root.clone(),
            OutputFormat::Human,
            4,
            no_cache,
            true,
            None,
        );
        config
            .resolve
            .conditions
            .clone_from(&self.options.conditions);
        config.cache_dir = if no_cache {
            self.scope().join("cold-cache")
        } else {
            self.cache_dir.clone()
        };
        config
    }

    /// Run each cached analysis with the persisted caches.
    pub fn observe_warm(&self) -> Observation {
        self.observe(false)
    }

    /// Run each analysis without a cache. This is the model of the warm run.
    pub fn observe_cold(&self) -> Observation {
        self.observe(true)
    }

    #[expect(
        deprecated,
        reason = "the graph cache lives behind fallow_core::analyze, the path that the cache oracle must drive"
    )]
    fn observe(&self, no_cache: bool) -> Observation {
        let config = self.config(no_cache);
        let analysis = fallow_core::analyze(&config).expect("analysis succeeds");
        let files = fallow_core::discover::discover_files(&config);
        let cache_dir = (!no_cache).then_some(config.cache_dir.as_path());
        let dupes = fallow_engine::duplicates::find_duplicates_with_defaults(
            &config.root,
            &files,
            &config.duplicates,
            cache_dir,
        );
        Observation {
            analysis: serde_json::to_value(&analysis).expect("serialize analysis"),
            dupes: serde_json::to_value(&dupes.report).expect("serialize dupes"),
        }
    }

    /// The bytes of the persisted graph cache, if a run wrote one.
    pub fn graph_cache_bytes(&self) -> Option<Vec<u8>> {
        std::fs::read(self.cache_dir.join("graph-cache.bin")).ok()
    }

    /// Every file below the warm cache directory with its bytes.
    pub fn cache_snapshot(&self) -> Vec<(PathBuf, Vec<u8>)> {
        let mut files = Vec::new();
        collect_files(&self.cache_dir, &mut files);
        files
            .into_iter()
            .map(|path| {
                let bytes = std::fs::read(&path).expect("read cache file");
                (path, bytes)
            })
            .collect()
    }

    /// Put the warm cache directory back to `snapshot`.
    pub fn restore_cache(&self, snapshot: &[(PathBuf, Vec<u8>)]) {
        let _ = std::fs::remove_dir_all(&self.cache_dir);
        for (path, bytes) in snapshot {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("create cache dir");
            }
            std::fs::write(path, bytes).expect("restore cache file");
        }
    }
}

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(path, content).expect("write project file");
}

fn set_times(path: &Path, time: SystemTime) {
    OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open file to set times")
        .set_times(FileTimes::new().set_accessed(time).set_modified(time))
        .expect("set file times");
}

fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, files);
        } else {
            files.push(path);
        }
    }
}
