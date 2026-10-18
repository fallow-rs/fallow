# Architecture overview

This page gives the system design and lists the external interfaces of Fallow.
Use it as the entry point, then follow the links for detail.

## Design

Fallow is a Rust analyzer for JavaScript and TypeScript.
It parses source with Oxc and does not use the TypeScript compiler.
The analysis pipeline has these stages:

```text
config -> discovery -> extract -> resolve -> graph -> analyze -> output
```

- [Repository map](development/repo-map.md): crate ownership and hotspots.
- [Architecture invariants](architecture-invariants.md): crate boundaries and dependency direction.
- [Detection internals](reference/detection-internals.md): extraction, graph and analysis behavior.

## External interfaces

| Interface | Direction | Contract |
| --- | --- | --- |
| CLI (`fallow`) | User and CI to Fallow | Commands, flags and exit codes: `0` success, `1` error-severity findings, `2` invalid input or execution error. |
| Config files | User to Fallow | Validated by `schema.json`. |
| Plugin files | User to Fallow | Validated by `plugin-schema.json` and `rule-pack-schema.json`. See [plugin authoring](plugin-authoring.md). |
| JSON, SARIF, CodeClimate, compact, markdown and badge output | Fallow to CI and tools | Typed output contract. See [output schema](output-schema.json). |
| LSP server (`fallow-lsp`) | Editor to Fallow | Diagnostics, code actions, code lens and hover. See [LSP internals](reference/lsp-internals.md). |
| MCP server (`fallow-mcp`) | Agent to Fallow | Tools over the shared analysis. See [MCP internals](reference/mcp-internals.md). |
| Node.js bindings | Node.js to Fallow | `crates/napi`, over the programmatic facade. |
| `fallow` npm package | Registry to user | Wrapper package with per-platform binary packages. |
| GitHub Action and GitLab CI template | CI to Fallow | `action.yml`, `action/` and `ci/`. |
| VS Code extension | Editor to Fallow | `editors/vscode`, a client of the LSP server. |

Output formats, the config schema and exit codes are public contracts.
[Backwards compatibility](backwards-compatibility.md) states the rules for changing them.

## Trust boundaries

[SECURITY.md](../SECURITY.md) describes the threat model, the build-time trust boundary and the release signing path.
