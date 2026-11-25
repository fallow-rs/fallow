//! An empty build script. It exists so that the feature-only `syn`
//! build-dependency in `Cargo.toml` takes part in the build, and keeps the
//! `syn` features equal between a package build and a workspace build.
//! The released skills are embedded by the build script of `fallow-api`.

fn main() {}
