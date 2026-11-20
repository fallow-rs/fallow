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
