//! `fallow-mcp` integration tests, built as one test binary.
//!
//! A separate test binary per file links the crate once per file. One
//! binary links it once. Add a new integration test as a module here, not as a new
//! file in `tests/`.

mod cloud_runtime_context;
mod inspect_target_cloud;
mod resources;
// The surface differential builds `file://` URIs from Unix paths.
#[cfg(unix)]
mod surface_differential;
mod typed_route_audit_base;
mod typed_route_env_coverage;
mod typed_route_env_production;
mod typed_route_env_production_tools;
mod version;
mod warm_session;

use std::path::{Path, PathBuf};

#[expect(
    clippy::expect_used,
    reason = "a test binary without its own path cannot run the tests"
)]
/// The Cargo profile directory of the running test binary. Older Cargo
/// puts test binaries in `<target>/<profile>/deps`. Newer Cargo puts them in
/// `<target>/<profile>/build/<pkg>/<hash>/out`. The `fallow` binary is in
/// `<target>/<profile>` for both, so this follows `CARGO_TARGET_DIR` and
/// `build.target-dir`.
fn cargo_profile_dir() -> PathBuf {
    let exe = std::env::current_exe().expect("test binary path");
    let dir = exe.parent().expect("test binary directory");
    dir.ancestors()
        .find(|ancestor| ancestor.ends_with("deps") || ancestor.ends_with("build"))
        .and_then(Path::parent)
        .unwrap_or(dir)
        .to_path_buf()
}

/// The `fallow` binary the MCP server shells out to. Pin it with `FALLOW_BIN`:
/// without the pin, the server falls back to a `fallow` on `PATH`, which can
/// be an older release. `cargo test --workspace` builds it; build it with
/// `cargo build -p fallow-cli` when you run the tests of this crate alone.
fn fallow_binary() -> PathBuf {
    let mut path = cargo_profile_dir().join("fallow");
    if cfg!(windows) {
        path.set_extension("exe");
    }
    assert!(
        path.is_file(),
        "fallow binary not found at {}. Build it first: cargo build -p fallow-cli",
        path.display()
    );
    path
}
