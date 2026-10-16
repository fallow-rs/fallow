# Threat model

`SECURITY.md` at the repository root is the full security policy. This file
gives the scanner the parts that matter for a scan.

## What this project does and where untrusted input enters

Fallow is a static analyzer for JavaScript and TypeScript projects. It parses
the files of a project and reports unused code, duplication, complexity and
architecture problems. Users often run it in CI on pull requests from people
they do not trust. Thus, the analyzed project is untrusted input.

Untrusted input enters through these paths:

- Source files under the project root: JS, TS, JSX, TSX, Vue, Svelte, Astro,
  MDX, CSS, SCSS and HTML. The parsers are in `crates/extract`.
- `package.json`, lockfiles, workspace manifests and `tsconfig.json`.
- Fallow config files (`.fallowrc.json`, `.fallowrc.jsonc`, `fallow.toml`),
  external plugin files (`.fallow/plugins/`, `fallow-plugin-*`) and rule packs.
  The parsers are in `crates/config`.
- Coverage input files: Istanbul JSON, V8 coverage and source maps.
- Git history, when a command uses `--changed-since` or churn data.
- JSON-RPC messages to the LSP server (`crates/lsp`) and the MCP server
  (`crates/mcp`). The client is an editor or an agent that shows the project.

## Security properties we promise

- Analysis never runs project code. A `package.json` lifecycle script is data.
- Analysis reads and writes only inside the project root, plus the fallow
  cache directory and report paths that the caller gives.
- A config glob with an absolute path or a `..` segment is an error (exit 2).
- Analysis makes no network request. Remote config `extends` is off unless the
  caller passes `--allow-remote-extends`. A repository config cannot turn it on.
- `fallow fix` edits files only inside the project root.
- The analysis crates (`fallow-core`, `fallow-extract`, `fallow-graph`) do not
  start processes. Git runs from `fallow-engine` with a clean environment.

A break of one of these properties is a vulnerability.

## Components that matter most / least

Most important:

- `crates/extract`, `crates/config`, `crates/graph`, `crates/core`,
  `crates/engine`: they read the untrusted project.
- `crates/cli`: path handling for `--root`, output paths, `fix`, and the CI
  report commands that post to GitHub or GitLab with a token from the
  environment.
- `crates/lsp` and `crates/mcp`: they take requests and turn them into
  analysis runs and file edits.

Less important, but in scope:

- `crates/napi`, `crates/api`: library bindings with the same analysis.
- `tools/type-aware-sidecar`: an optional Node.js companion program.
- `tools/similar-code-sidecar`: an optional Rust companion program in its own
  Cargo workspace. This image does not build it.

Out of scope:

- `benchmarks/`, `tests/fixtures/`, `examples/`, `fuzz/corpus/`: test data.
  Some fixtures contain deliberate bad code.
- `editors/`, `viz-frontend/`, `apps/`, `npm/`, `action/`, `ci/`: not part
  of this build.

## How to exercise it

- `target/debug/fallow --root <dir> --format json --quiet` runs the full
  analysis on a directory. `fallow --help` lists all commands.
- `tests/fixtures/` holds many small projects. Copy one and change it.
- `target/debug/fallow-lsp` and `target/debug/fallow-mcp` speak JSON-RPC on
  stdin and stdout.
- `cargo test --offline -p <crate>` runs the tests of one crate.
- `fuzz/fuzz_targets/` shows the parser entry points. The fuzz harness needs a
  nightly toolchain, which this image does not have.

## How you rate severity

- Critical: code execution on the host from an analyzed project, with the
  default flags.
- High: a read or a write outside the project root, a network request without
  `--allow-remote-extends`, a bypass of config glob validation, or a leak of a
  CI token.
- Medium: memory unsafety without a shown exploit, or a crash or hang from a
  small input that stops a CI run.
- Low: a crash or a slow run from a very large or deep input, such as a file of
  many megabytes or nesting deeper than real code.

A panic is a bug, not a vulnerability, unless it has an effect from the list
above. Report it as low.

## Anything to leave alone

- `fallow mutation` without `--plan` runs the test suite of the project. This
  runs project code by design, after the user starts it. Report only a run
  that starts without that command, or a break of its documented safety
  boundary (`docs/mutation-testing.md`).
- `--allow-remote-extends` fetches HTTPS URLs by design.
- Runtime coverage commands upload data to a service by design, after the user
  gives a token.
- Files under `.claude/`, `.agents/` and `docs/` are maintainer documentation.
