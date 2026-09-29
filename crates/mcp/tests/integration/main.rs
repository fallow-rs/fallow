//! `fallow-mcp` integration tests, built as one test binary.
//!
//! Each module used to be a test binary of its own. One binary links the
//! crate once. Add a new integration test as a module here, not as a new
//! file in `tests/`.

mod cloud_runtime_context;
mod resources;
mod typed_route_env_coverage;
mod version;
mod warm_session;

use std::path::PathBuf;

#[allow(
    clippy::expect_used,
    reason = "a test binary without its own path cannot run the tests"
)]
/// The Cargo profile directory of the running test binary. Test binaries
/// live in `<target>/<profile>/deps`, next to the `fallow` binary one level
/// up, so this follows `CARGO_TARGET_DIR` and `build.target-dir`.
fn cargo_profile_dir() -> PathBuf {
    let exe = std::env::current_exe().expect("test binary path");
    let dir = exe.parent().expect("test binary directory");
    if dir.ends_with("deps") {
        dir.parent().expect("profile directory").to_path_buf()
    } else {
        dir.to_path_buf()
    }
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
