# `security`: Security Candidate Detection

Surfaces local security candidates for agent or human verification. The first rule, `client-server-leak`, starts at `"use client"` files and reports a candidate when that client boundary directly reads, or statically imports a path to a module that reads, a non-public `process.env` value.

Findings are not confirmed vulnerabilities. Use the structural trace to verify whether the value can actually reach client-bundled code. Public env conventions (`NODE_ENV`, `NEXT_PUBLIC_*`, `VITE_*`, `NUXT_PUBLIC_*`, `REACT_APP_*`, `PUBLIC_*`, `GATSBY_*`, `EXPO_PUBLIC_*`, `STORYBOOK_*`) are excluded.

The second rule family is a data-driven `tainted-sink` catalogue: syntactic dangerous-sink candidates across the catalogue categories listed below. Most rows require a non-literal argument; narrowly literal-aware rows flag deterministic unsafe literals such as wildcard `postMessage` origins, weak crypto algorithms, disabled TLS validation, and JWT algorithm issues. Fallow prefers false-negatives over false-positives.

| Category | CWE | Sink |
|----------|-----|------|
| `dangerous-html` | 79 | `innerHTML` / `outerHTML` / `insertAdjacentHTML` / `dangerouslySetInnerHTML` / iframe `srcdoc` (property, `setAttribute`, JSX `srcDoc`, Vue / Svelte / Angular bindings) / `Range.createContextualFragment` |
| `template-escape-bypass` | 79 | template-engine `SafeString(...)` wrapping a non-literal value |
| `command-injection` | 78 | `child_process` `exec` / `execSync` / `spawn` / `spawnSync` (provenance-gated to `node:child_process`) |
| `code-injection` | 94 | `eval` (also `(0, eval)`, `globalThis.eval`, a local alias) / `Function` constructor forms / string `setTimeout` / `vm.runInNewContext`, `vm.compileFunction` and `new vm.Script` (also a named or `require` binding) / `<script>` element `src` (property or `setAttribute`) and text |
| `dynamic-regex` | 1333 | `RegExp(...)` / `new RegExp(...)` with a non-literal pattern that traces to untrusted input |
| `redos-regex` | 1333 | vulnerable regex literals tested with source-backed input |
| `resource-amplification` | 400 | source-backed size into `Array(...)` / `new Array(...)` / `Buffer.alloc*` / `String.prototype.repeat` / `padStart` / `padEnd` (directly `Math.min`-clamped sizes stay quiet) |
| `dynamic-module-load` | 95 | dynamic `require(...)` |
| `sql-injection` | 89 | string concat or interpolated template into `.query()` / `.execute()`, and `sql.raw(...)`. Parameterized `` sql`${x}` `` and the object form `.execute({ sql, args })` are NOT flagged |
| `ssrf` | 918 | `fetch` / `got` / `ky` / `needle` / `request` / `axios` / `superagent` / `undici` / `http(s).request` |
| `path-traversal` | 22 | `path.join` / `path.resolve` with untrusted input in the argument, the same module or every direct importer / `node:fs` path methods / route `sendFile` |
| `header-injection` | 113 | response `setHeader` / `writeHead` with a value that traces to untrusted input |
| `open-redirect` | 601 | `res.redirect` / `location.href` / `location.assign` / `window.open` |
| `postmessage-wildcard-origin` | 346 | `postMessage(..., "*")` |
| `tls-validation-disabled` | 295 | HTTPS/TLS options with `rejectUnauthorized: false`, plus `NODE_TLS_REJECT_UNAUTHORIZED = "0"` |
| `cleartext-transport` | 319 | cleartext `http://` URLs in fetch-like calls and WebSocket constructors |
| `electron-unsafe-webpreferences` | 1188 | Electron `webPreferences` with unsafe literal options |
| `world-writable-permission` | 732 | `chmod` / `chmodSync` with world-writable modes |
| `insecure-temp-file` | 377 | predictable temporary file paths in `fs` writes |
| `mysql-multiple-statements` | 89 | MySQL connection options with `multipleStatements: true` |
| `permissive-cors` | 942 | CORS wildcard origin with credentials |
| `insecure-cookie` | 614 | cookie options missing or disabling `httpOnly` / `secure` |
| `mass-assignment` | 915 | source-backed `Object.assign(target, source)` |
| `weak-crypto` | 327 | runtime-selectable hash / cipher algorithm |
| `deprecated-cipher` | 327 | `crypto.createCipher` / `createDecipher` |
| `insecure-randomness` | 338 | `crypto.pseudoRandomBytes(...)` and token-like `Math.random()` use |
| `jwt-alg-none` | 347 | JWT signing with algorithm `none` |
| `jwt-verify-missing-algorithms` | 347 | `jsonwebtoken` verify calls missing an `algorithms` allowlist |
| `unsafe-buffer-alloc` | 1188 | `Buffer.allocUnsafe` / `allocUnsafeSlow` |
| `unsafe-deserialization` | 502 | `js-yaml` `load` / `node-serialize` |
| `angular-trusted-html` | 79 | Angular `bypassSecurityTrust*` |
| `nextjs-open-redirect` | 601 | Next.js `redirect` / `permanentRedirect` |
| `react-router-open-redirect` | 601 | React Router `redirect` / `redirectDocument` / `replace` |
| `dom-document-write` | 79 | `document.write` / `document.writeln` |
| `jquery-html` | 79 | jQuery `.html(value)` |
| `route-send-file` | 22 | Express / Fastify / Hono route `sendFile` |
| `webview-injection` | 94 | react-native-webview injected JavaScript |
| `prototype-pollution` | 1321 | `__proto__` writes and recursive merge sources |
| `zip-slip` | 22 | archive extraction destination paths |
| `nosql-injection` | 943 | Mongo / Mongoose query object passthrough |
| `ssti` | 1336 | template engine compile / render calls |
| `xxe` | 611 | XML parse calls |
| `secret-pii-log` | 532 | source-backed secrets or request PII reaching logs |
| `hardcoded-secret` | 798 | provider-prefix credentials and high-entropy literals assigned to secret-shaped identifiers (include-required) |
| `secret-to-network` | 201 | a non-public `process.env` / `import.meta.env` secret reaching a network call body (`fetch` / `axios` / `got` / ...) via same-identifier flow (include-required) |
| `llm-call-injection` | 1427 | an untrusted source reaching the prompt/messages argument of a known LLM-call sink (taint-path gated, pinned to distinctive LLM SDK call shapes) |
| `xpath-injection` | 643 | `xpath.select` / `select1` with a non-literal expression |

Build-config and test files are excluded from candidate generation. Security rule families default to `off` and are surfaced only by `fallow security`, never under bare `fallow` or the `audit` gate. Scope which catalogue categories run with `security.categories` include / exclude lists in config. Add project-local request object names with `security.requestReceivers`; it extends the built-in `req` / `request` / `ctx` / `context` / `event` allowlist for HTTP `query`, `params`, and `body` reads. The setting is additive only and does not gate `*.searchParams`. `hardcoded-secret` and `secret-to-network` are intentionally include-required and only run when listed in `security.categories.include` (`secret-to-network` is opt-in because legitimate auth is also a secret reaching a network call). Public-by-convention env vars (`NEXT_PUBLIC_`, `VITE_`, ...) are never treated as secrets.

## Flags

<!-- generated:flags:security:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| `--runtime-coverage` | `string` | - | Runtime coverage input. Accepts a V8 directory, a single V8 JSON file, or an Istanbul coverage map JSON. When set, `fallow security` adds production runtime state to tainted-sink candidates and uses that state as an extra ranking signal. A single local capture is free. Continuous or multi-capture monitoring needs a license (see `fallow license`) |
| `--min-invocations-hot` | `string` | `100` | Threshold for hot-path classification, forwarded to the sidecar when `--runtime-coverage` is set |
| `--file` | `string` | - | Scope output to candidates whose finding anchor or trace hop matches the selected file. The full graph is still analyzed |
| `--gate` | `new\|newly-reachable` | - | `new` fails (exit code **8**) only when the change introduces a NEW security-sink candidate in the changed lines. It requires a diff source (`--changed-since`, `--diff-file`, or `--diff-stdin`). `newly-reachable` fails when an existing candidate becomes reachable from entry points compared with `--changed-since <ref>`; diff-only inputs exit 2 because this mode analyzes the base tree. Human output says `REVIEW REQUIRED` (not `FAIL`); SARIF keeps every result at `level: note` with the verdict in `run.properties.fallowGate`; `--format json` carries an additive `gate` block (`mode` / `verdict` / `new_count`) |
| `--surface` | `bool` | `false` | Include the agent-facing `attack_surface[]` inventory in JSON output |

Common global flags for this command: [`--format`](global-flags.md), [`--quiet`](global-flags.md), [`--changed-since`](global-flags.md), [`--diff-file`](global-flags.md), [`--diff-stdin`](global-flags.md), [`--workspace`](global-flags.md), [`--changed-workspaces`](global-flags.md).
<!-- generated:flags:security:end -->
## Examples

```bash
fallow security --format json --quiet
fallow security --ci --sarif-file fallow-security.sarif
git diff --unified=0 origin/main...HEAD | fallow security --diff-file -
# Regression gate: fail (exit 8) only on candidates introduced in the changed lines
fallow security --gate new --changed-since origin/main
git diff --cached --unified=0 | fallow security --gate new --diff-stdin

# Reachability gate: fail when existing sinks become entry-point reachable
fallow security --gate newly-reachable --changed-since origin/main
```

## JSON Output Structure

```json
{
  "kind": "security",
  "schema_version": "4",
  "version": "3.49.0",
  "elapsed_ms": 42,
  "config": {
    "rules": {
      "security_client_server_leak": {
        "configured": "off",
        "effective": "warn"
      },
      "security_sink": {
        "configured": "off",
        "effective": "warn"
      }
    },
    "categories_include": null,
    "categories_exclude": null
  },
  "security_findings": [],
  "unresolved_edge_files": 0,
  "unresolved_callee_sites": 0,
  "unresolved_callee_diagnostics": null
}
```

`fallow security --summary --format json --quiet` emits the same `kind`, `schema_version`, `version`, `elapsed_ms`, and `config` metadata, but replaces candidate arrays with `summary` aggregate counts:

```json
{
  "kind": "security",
  "schema_version": "4",
  "version": "3.49.0",
  "elapsed_ms": 42,
  "config": {
    "rules": {
      "security_client_server_leak": {
        "configured": "off",
        "effective": "warn"
      },
      "security_sink": {
        "configured": "off",
        "effective": "warn"
      }
    },
    "categories_include": null,
    "categories_exclude": null
  },
  "summary": {
    "security_findings": 0,
    "by_severity": {
      "high": 0,
      "medium": 0,
      "low": 0
    },
    "by_category": {},
    "by_reachability": {
      "entry_reachable": 0,
      "untrusted_source_reachable": 0,
      "arg_level": 0,
      "module_level": 0,
      "crosses_boundary": 0,
      "source_backed": 0
    },
    "by_runtime_state": {
      "runtime_hot": 0,
      "runtime_cold": 0,
      "never_executed": 0,
      "low_traffic": 0,
      "coverage_unavailable": 0,
      "runtime_unknown": 0,
      "not_collected": 0
    },
    "unresolved_edge_files": 0,
    "unresolved_callee_sites": 0,
    "attack_surface_entries": 0
  }
}
```

Each finding includes `kind`, `path`, `line`, `col`, `evidence`, `trace`, `actions`, `severity`, and optional `reachability`. `severity` is a review-priority tier (`high`, `medium`, or `low`) derived from reachability, boundary, source-backed, and runtime-hot signals; it is not a verified vulnerability verdict and does not change gate or exit semantics. SARIF maps high and medium candidates to `warning`, and low candidates to `note`. `tainted-sink` findings additionally carry `category` (the catalogue id, e.g. `"dangerous-html"`) and `cwe`. `client-server-leak` findings carry `category` (`"client-server-leak"` for a secret leak, `"server-only-import"` for server-only code) and omit `cwe`. `tainted-sink` findings can also include `reachability.untrusted_source_trace` when a module with a known untrusted source imports the sink module (a test, story or tooling-config file, and a `process.env` secret, never start this trace); it is ranking and triage context only, not proof that a specific value reaches the sink. When set, `reachability.taint_confidence` tiers the association as `"arg-level"` (the sink argument traces to a same-module source read, strong) or `"module-level"` (only the module is import-reachable from a source, weak); tier from this field rather than the evidence text. For arg-level findings the trace's first hop points at the actual source-read line, and module-level source hops carry the role `"module-source"`. `unresolved_edge_files` (client-server-leak) and `unresolved_callee_sites` (tainted-sink) are in-band blind-spot counters: a zero finding count with a non-zero counter is not a clean bill. When no analyzed file has a top-level `"use client"` directive, `workspace_diagnostics[]` carries `client-server-leak-no-client-roots` and the human output prints a blind-spot line: the rule had no starting point, so its zero count measures nothing. Suppress a verified false positive with `// fallow-ignore-file security-client-server-leak` (client-server-leak) or `// fallow-ignore-file security-sink` (any tainted-sink category).

When present, `unresolved_callee_diagnostics` adds bounded unresolved-callee metadata for follow-up review: `sampled[]` rows with `path`, `line`, `col`, `reason`, and `expression_kind`, `top_files[]` counts, `by_reason[]` counts, and the emitted sample/top-file limits. It is blind-spot metadata, not a finding list, and follows the same `--file`, `--workspace`, `--changed-since`, and `--gate new` scoping as security candidates.

Every finding also carries an agent-actionable `candidate { source_kind, sink, boundary }`, an optional `taint_flow { source, sink, path }`, a `finding_id` and an optional `stable_finding_id`:

- `candidate.source_kind`: the untrusted-input kind that reaches the sink, as a stable catalogue id (`"http-request-input"`, `"process-env"`, `"process-argv"`, `"message-event-data"`, `"location-input"`, ...). Absent when no source matched (always absent for `client-server-leak`). Treat an unknown id as an untrusted source of unknown kind; never drop the candidate on that basis.
- `candidate.sink`: a self-contained sink (`path`, `line`, `col`, `category`, `cwe`, `callee`, optional `url_shape`), actionable without reading the rest of the finding. URL-category sinks use `url_shape` to distinguish `fixed-origin-dynamic-path` from `dynamic-origin` when the construction is statically visible.
- `candidate.boundary`: `client_server` (a `"use client"` file in the trace), `cross_module` (the source reaches the sink across import hops), and optional `architecture_zone` (`from`/`to`) when the anchor also crosses a declared architecture boundary.
- `candidate.network`: present only on `secret-to-network` (#890) candidates. `destination` is the network call's URL when it is a static literal (usually intended auth) or absent when the destination is dynamic (the higher-signal exfil case). Use it to triage exfil from intended auth without re-reading source.
- There is no `impact` field: deciding exploitability is the verifying agent's job; `severity` is only the review-priority tier.
- `taint_flow`: present only when an untrusted source is import-reachable to the sink. `path` is the compact `{ intra_module, cross_module_hops }` shape; the full ordered hops stay in `reachability.untrusted_source_trace`.
- `finding_id`: a location id, identical across runs for the same rule/path/line/column and identical to SARIF `partialFingerprints["fallowSecurity/v2"]`. Use it to join JSON with SARIF. An edit above the sink changes it. On upgrading from line-only IDs, regenerate candidates and their verdicts together, including ID-based evaluation labels. Saved candidate/verdict pairs from the same version remain usable; old review history does not transfer automatically to the new IDs.
- `stable_finding_id`: a line-independent id of the form `sec1:<16 hex digits>`. It hashes the rule, the project-relative path and the exact text of the full sink expression (the full assignment, or the full call with all arguments), so whitespace, comments, line breaks, trailing commas and an edit above the sink do not change it. A new quote style changes the text of a string literal and gives a new id. A hard-coded secret hashes its binding name and provider family, never the value. A client-server leak hashes the paths of its first and last trace hop and the names of the env secrets that it reads. Identical sinks in one file get the suffix `~1`, `~2` and so on; an inline suppression does not move these suffixes. Key stored verdicts by this id. `fallow security survivors` accepts it or `finding_id` in a verdict. The field is absent for a sink in Vue, Svelte or Angular template markup and when fallow cannot read the anchor file; use `finding_id` then.
