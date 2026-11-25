use super::{Path, session_store, watched_file_globs};

#[test]
fn watched_file_globs_cover_every_discovered_source_extension() {
    let patterns: Vec<_> = watched_file_globs()
        .iter()
        .map(|glob| {
            fallow_config::extglob::ExtglobMatcher::new(glob, &[]).expect("valid watcher glob")
        })
        .collect();
    for extension in fallow_api::SOURCE_EXTENSIONS {
        let path = format!("src/example.{extension}");
        assert!(
            patterns.iter().any(|glob| glob.is_match(Path::new(&path))),
            "no watcher for {path}"
        );
    }
}

#[test]
fn watched_file_globs_cover_every_config_file_name() {
    let globs = watched_file_globs();

    for name in fallow_config::CONFIG_FILE_NAMES {
        let expected = format!("**/{name}");
        assert!(
            globs.contains(&expected),
            "{name} is a config file the loader reads, so the watcher must register {expected}: {globs:?}"
        );
    }
}

#[test]
fn watched_file_globs_cover_every_near_miss_config_file_name() {
    let globs = watched_file_globs();

    for name in fallow_config::UNRECOGNIZED_CONFIG_FILE_NAMES {
        let expected = format!("**/{name}");
        assert!(
            globs.contains(&expected),
            "a run reports a notice for {name}, so the watcher must register {expected}: {globs:?}"
        );
        assert!(
            session_store::session_input_file(Path::new(name)),
            "a created or removed {name} must load the kept sessions again"
        );
    }
}

#[test]
fn workspace_manifests_are_watched_session_inputs() {
    let globs = watched_file_globs();

    for name in ["pnpm-workspace.yaml", "deno.json", "deno.jsonc"] {
        let expected = format!("**/{name}");
        assert!(
            globs.contains(&expected),
            "workspace discovery reads {name}, so the watcher must register {expected}: {globs:?}"
        );
        assert!(
            session_store::session_input_file(Path::new(name)),
            "an edit to {name} must load the kept sessions again"
        );
    }
}
