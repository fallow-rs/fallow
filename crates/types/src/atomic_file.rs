//! Atomic replacement of a cache file.
//!
//! The extraction cache and the graph cache both live in `.fallow/`, and more
//! than one fallow process can write there at the same time: an editor server
//! beside a CLI run, or a pre-commit hook beside an editor. A fixed temp name
//! (`cache.bin.tmp`) let those writers truncate and interleave bytes in one
//! shared file, so the renamed result could hold parts of two payloads, and a
//! writer whose temp file another writer had already renamed failed outright.
//! Each write here gets its own uniquely named temp file in the target
//! directory, so the rename always moves one complete payload into place.

use std::io::Write as _;
use std::path::Path;

/// The mode `File::create` requests on Unix, before the umask applies.
#[cfg(unix)]
const DEFAULT_FILE_MODE: u32 = 0o666;

/// Replace `path` with `data` atomically.
///
/// The data goes to a uniquely named temp file beside `path`, gets a
/// best-effort `fsync`, and is then renamed over `path`. Readers see the old
/// file or the new file, never a mix. Concurrent writers each move their own
/// complete payload into place, and the last rename wins. A failed write
/// removes its temp file.
///
/// # Errors
///
/// Returns the IO error when `path` has no parent directory or file name, or
/// when the temp file cannot be created, written, or renamed into place.
pub fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "cache file path has no parent directory or file name",
        ));
    };
    let dir = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    let mut prefix = name.to_os_string();
    prefix.push(".");
    let mut builder = tempfile::Builder::new();
    builder.prefix(&prefix).suffix(".tmp");
    // A temp file is private (0600) by default. Ask for the mode that
    // `File::create` uses, so the umask decides as before and a cache that
    // another user or a CI container reads stays readable.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        builder.permissions(std::fs::Permissions::from_mode(DEFAULT_FILE_MODE));
    }
    let mut tmp = builder.tempfile_in(dir)?;
    tmp.write_all(data)?;
    // Best effort: a filesystem without fsync support still gets the rename.
    let _ = tmp.as_file().sync_all();
    persist(tmp, path)
}

/// How often a rename over `path` is tried when another writer holds it.
#[cfg(windows)]
const PERSIST_ATTEMPTS: u32 = 20;

/// The pause between two rename attempts.
#[cfg(windows)]
const PERSIST_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(5);

#[cfg(not(windows))]
fn persist(tmp: tempfile::NamedTempFile, path: &Path) -> std::io::Result<()> {
    tmp.persist(path).map_err(|error| error.error)?;
    Ok(())
}

/// Windows refuses a rename over a file that another writer is replacing at
/// the same moment (`ERROR_ACCESS_DENIED`). The other rename finishes within
/// milliseconds, so a short bounded retry lets each writer move its payload
/// into place.
#[cfg(windows)]
fn persist(mut tmp: tempfile::NamedTempFile, path: &Path) -> std::io::Result<()> {
    let mut attempt = 1;
    loop {
        match tmp.persist(path) {
            Ok(_) => return Ok(()),
            Err(error)
                if error.error.kind() == std::io::ErrorKind::PermissionDenied
                    && attempt < PERSIST_ATTEMPTS =>
            {
                tmp = error.file;
                attempt += 1;
                std::thread::sleep(PERSIST_RETRY_DELAY);
            }
            Err(error) => return Err(error.error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir_names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[cfg_attr(miri, ignore = "the filesystem is blocked by Miri isolation")]
    #[test]
    fn write_replaces_the_file_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.bin");
        write_atomic(&path, b"first").unwrap();
        write_atomic(&path, b"second payload").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second payload");
        assert_eq!(dir_names(dir.path()), ["cache.bin"]);
    }

    #[cfg_attr(miri, ignore = "the filesystem is blocked by Miri isolation")]
    #[test]
    fn concurrent_writers_each_move_one_whole_payload_into_place() {
        const WRITERS: u8 = 8;
        const ROUNDS: usize = 50;
        const PAYLOAD_LEN: usize = 256 * 1024;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.bin");
        let errors = std::sync::Mutex::new(Vec::<std::io::Error>::new());
        std::thread::scope(|scope| {
            for writer in 0..WRITERS {
                let (path, errors) = (&path, &errors);
                scope.spawn(move || {
                    let payload = vec![writer; PAYLOAD_LEN];
                    for _ in 0..ROUNDS {
                        if let Err(error) = write_atomic(path, &payload) {
                            errors.lock().unwrap().push(error);
                        }
                    }
                });
            }
        });
        let errors = errors.into_inner().unwrap();
        assert!(errors.is_empty(), "writes failed: {errors:?}");

        let data = std::fs::read(&path).unwrap();
        assert_eq!(data.len(), PAYLOAD_LEN, "the file holds one whole payload");
        assert!(
            data.iter().all(|byte| *byte == data[0]),
            "the file holds bytes from more than one writer"
        );
        assert_eq!(dir_names(dir.path()), ["cache.bin"]);
    }

    /// A cache that another user or a CI container reads must keep the mode a
    /// plain `File::create` gives, not the private mode of a temp file.
    #[cfg(unix)]
    #[cfg_attr(miri, ignore = "the filesystem is blocked by Miri isolation")]
    #[test]
    fn the_file_gets_the_mode_that_file_create_gives() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let reference = dir.path().join("reference");
        std::fs::File::create(&reference).unwrap();
        let path = dir.path().join("cache.bin");
        write_atomic(&path, b"data").unwrap();
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), mode(&reference));
    }

    #[test]
    fn a_path_without_a_file_name_is_refused() {
        let error = write_atomic(Path::new("/"), b"data").unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }
}
