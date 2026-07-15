use std::path::{Path, PathBuf};

#[expect(
    clippy::redundant_pub_crate,
    reason = "shared by sibling LSP modules through the private path_utils module"
)]
pub(crate) fn canonicalize_for_lsp(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}
