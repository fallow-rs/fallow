use super::ensure_gitignore;

fn gitignore_after(seed: Option<&[u8]>) -> (tempfile::TempDir, Vec<u8>) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".gitignore");
    if let Some(seed) = seed {
        std::fs::write(&path, seed).unwrap();
    }
    ensure_gitignore(dir.path());
    let bytes = std::fs::read(&path).unwrap();
    (dir, bytes)
}

#[test]
fn a_latin1_gitignore_keeps_its_rules() {
    let (_dir, bytes) = gitignore_after(Some(b"caf\xe9.txt\nnode_modules/"));
    assert_eq!(bytes, b"caf\xe9.txt\nnode_modules/\n.fallow/\n");
}

#[test]
fn a_latin1_gitignore_with_the_entry_is_unchanged() {
    let seed: &[u8] = b"caf\xe9.txt\n.fallow/\n";
    let (_dir, bytes) = gitignore_after(Some(seed));
    assert_eq!(bytes, seed);
}

#[test]
fn a_crlf_gitignore_with_the_entry_is_unchanged() {
    let seed: &[u8] = b"node_modules/\r\n.fallow\r\n";
    let (_dir, bytes) = gitignore_after(Some(seed));
    assert_eq!(bytes, seed);
}

#[test]
fn a_missing_gitignore_is_created() {
    let (_dir, bytes) = gitignore_after(None);
    assert_eq!(bytes, b".fallow/\n");
}

#[test]
fn a_gitignore_that_cannot_be_read_is_not_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".gitignore");
    std::fs::create_dir(&path).unwrap();
    ensure_gitignore(dir.path());
    assert!(path.is_dir());
}

/// A `.gitignore` that links to a file in `node_modules` is not changed.
#[cfg(unix)]
#[test]
fn a_gitignore_link_into_node_modules_is_not_changed() {
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("node_modules/left-pad");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(package.join("index.js"), b"module.exports = 1;\n").unwrap();
    std::os::unix::fs::symlink(package.join("index.js"), dir.path().join(".gitignore")).unwrap();
    ensure_gitignore(dir.path());
    assert_eq!(
        std::fs::read(package.join("index.js")).unwrap(),
        b"module.exports = 1;\n"
    );
}

/// A `.gitignore` link to another project file stays writable.
#[cfg(unix)]
#[test]
fn a_gitignore_link_to_a_project_file_is_updated() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("config")).unwrap();
    std::fs::write(dir.path().join("config/ignore"), b"dist/\n").unwrap();
    std::os::unix::fs::symlink(
        dir.path().join("config/ignore"),
        dir.path().join(".gitignore"),
    )
    .unwrap();
    ensure_gitignore(dir.path());
    assert_eq!(
        std::fs::read(dir.path().join("config/ignore")).unwrap(),
        b"dist/\n.fallow/\n"
    );
}
