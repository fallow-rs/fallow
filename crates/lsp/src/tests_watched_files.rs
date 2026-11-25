use super::{
    DidChangeWatchedFilesParams, DidSaveTextDocumentParams, FallowLspServer, FileChangeType,
    FileEvent, LanguageServer, LspService, Ordering, Path, TextDocumentIdentifier, Uri,
    session_store, watched_file_globs,
};

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

fn write_source(dir: &Path, name: &str, content: &str) -> Uri {
    let path = dir.join(name);
    std::fs::write(&path, content).expect("write source");
    Uri::from_file_path(&path).expect("file uri")
}

async fn send_watched(backend: &FallowLspServer, uri: &Uri, typ: FileChangeType) {
    backend
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent::new(uri.clone(), typ)],
        })
        .await;
}

fn epoch(backend: &FallowLspServer) -> u64 {
    backend.analysis_epoch.load(Ordering::SeqCst)
}

fn quiet_backend() -> (LspService<FallowLspServer>, tower_lsp_server::ClientSocket) {
    let (service, socket) = LspService::build(FallowLspServer::new).finish();
    // No run starts at once, so the test reads the epoch of the events only.
    service
        .inner()
        .startup_analysis_started
        .store(true, Ordering::SeqCst);
    (service, socket)
}

// A watcher that keeps reporting a file whose content does not change made
// each report start a new run, so the server never came to rest (#5278).
#[tokio::test(flavor = "current_thread")]
async fn repeated_watched_events_for_unchanged_content_start_one_run() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (service, _socket) = quiet_backend();
    let backend = service.inner();
    let uri = write_source(dir.path(), ".fallowrc.json", "{}\n");

    let before = epoch(backend);
    for _ in 0..5 {
        send_watched(backend, &uri, FileChangeType::CHANGED).await;
    }
    assert_eq!(
        epoch(backend),
        before + 1,
        "only the first event is a change"
    );
}

// One editor save sends `didSave` and then a watched-file event for the same
// content. The second event cancelled the run of the first one.
#[tokio::test(flavor = "current_thread")]
async fn the_watched_event_of_a_save_starts_no_second_run() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (service, _socket) = quiet_backend();
    let backend = service.inner();
    let uri = write_source(dir.path(), "a.ts", "export const a = 1;\n");

    let before = epoch(backend);
    backend
        .did_save(DidSaveTextDocumentParams {
            text_document: TextDocumentIdentifier::new(uri.clone()),
            text: None,
        })
        .await;
    send_watched(backend, &uri, FileChangeType::CHANGED).await;
    assert_eq!(epoch(backend), before + 1);
}

#[tokio::test(flavor = "current_thread")]
async fn a_watched_event_with_new_content_starts_a_run() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (service, _socket) = quiet_backend();
    let backend = service.inner();
    let uri = write_source(dir.path(), "a.ts", "export const a = 1;\n");
    send_watched(backend, &uri, FileChangeType::CHANGED).await;

    let before = epoch(backend);
    write_source(dir.path(), "a.ts", "export const a = 2;\n");
    send_watched(backend, &uri, FileChangeType::CHANGED).await;
    assert_eq!(epoch(backend), before + 1);
}

#[tokio::test(flavor = "current_thread")]
async fn a_deleted_and_created_file_starts_a_run_each_time() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (service, _socket) = quiet_backend();
    let backend = service.inner();
    let uri = write_source(dir.path(), "a.ts", "export const a = 1;\n");
    send_watched(backend, &uri, FileChangeType::CHANGED).await;

    let before = epoch(backend);
    std::fs::remove_file(dir.path().join("a.ts")).expect("remove");
    send_watched(backend, &uri, FileChangeType::DELETED).await;
    write_source(dir.path(), "a.ts", "export const a = 1;\n");
    send_watched(backend, &uri, FileChangeType::CREATED).await;
    assert_eq!(epoch(backend), before + 2);
}

// A client can report one event for a deleted directory and none for its
// files. A file created again with its old content must still start a run.
#[tokio::test(flavor = "current_thread")]
async fn a_created_file_with_known_content_starts_a_run() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir(dir.path().join("src")).expect("src dir");
    let (service, _socket) = quiet_backend();
    let backend = service.inner();
    let uri = write_source(&dir.path().join("src"), "a.ts", "export const a = 1;\n");
    send_watched(backend, &uri, FileChangeType::CHANGED).await;

    let before = epoch(backend);
    send_watched(backend, &uri, FileChangeType::CREATED).await;
    assert_eq!(epoch(backend), before + 1);

    send_watched(backend, &uri, FileChangeType::CHANGED).await;
    assert_eq!(
        epoch(backend),
        before + 1,
        "the created event records the file"
    );

    let src = Uri::from_file_path(dir.path().join("src")).expect("dir uri");
    send_watched(backend, &src, FileChangeType::DELETED).await;
    send_watched(backend, &uri, FileChangeType::CHANGED).await;
    assert_eq!(
        epoch(backend),
        before + 3,
        "the directory delete forgets the record of its files",
    );
}
