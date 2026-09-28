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
