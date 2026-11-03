### Added

- **The MCP server analyzes a Rust workspace through `cargo fallow`.** A root with a `Cargo.toml` and no `package.json` takes the Rust route. Each routed tool runs `cargo-fallow` and returns its report with `language: "rust"`.
  - `analyze` runs `cargo fallow check --only deps --only visibility`. The partial run has no health score.
  - `find_dupes`, `check_health` and `check_architecture` run `cargo fallow dupes`, `complexity` and `boundaries`. On this route, `check_architecture` returns `kind: "boundaries"`, and `check_health` has no score.
  - `mutation_plan` runs `cargo fallow mutants --list`. Without `paths` or `changed_since`, the plan covers the whole package, not the changed code. `mutation_inspect` runs `cargo fallow mutants --inspect <id> --no-build`, which reads only the cached coverage map. Both tools have a new `package` parameter, and neither tool compiles or runs tests.
  - A parameter that the Rust route cannot honor gives `FALLOW_RUST_UNSUPPORTED_PARAMS`. `project_info` and `code_execute` give `FALLOW_RUST_UNSUPPORTED_TOOL`. A missing binary gives `FALLOW_RUST_BINARY_NOT_FOUND`. Set `CARGO_FALLOW_BIN` to name the binary.
  - A root with both manifests keeps the JavaScript route, and the result gets a `warnings` entry that points to `cargo fallow`.
  - The server instructions and the descriptions of the routed tools name the Rust route. MCP clients that cache `tools/list` get new descriptions.
