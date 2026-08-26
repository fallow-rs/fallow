# Similar-code analysis

`fallow similar-code` is an opt-in, local semantic discovery workflow for
JavaScript and TypeScript functions. It complements the deterministic `dupes`
detector. `dupes` finds copied structure and near clones, while `similar-code`
finds functions that may express the same intent even when names, syntax, and
control flow differ.

The output is advisory. A model score is not a probability, proof of behavioral
equivalence, or refactor-safety decision. Raw results are always marked
`unverified` and never affect the normal Fallow exit status, audit gate, SARIF,
LSP diagnostics, VS Code diagnostics, or auto-fix.

## User flow

The exact-version `fallow-similar-code` companion ships with the npm platform
package. The first model download requires an explicit human decision:

```bash
npx fallow similar-code status
npx fallow similar-code setup --local
```

For a non-interactive setup that the user has already approved, add `--yes`.
Project config, MCP, Node API calls, and model output cannot authorize setup.

Run discovery and save its independent JSON envelope:

```bash
npx fallow similar-code --format json --quiet > similar-code.json
```

Inference stays local and offline. A cold run may take several minutes on a
large repository because every admitted function needs an embedding. Later
runs reuse a user-local vector cache namespaced by the canonical project root.
File, changed-file, diff, and workspace scopes prioritize matching functions
before hard limits and compare only pairs with at least one fully matching
endpoint. A deterministic background sample preserves useful context inside the
comparison budget. `--top` limits only the returned candidates.

Each `candidates[]` entry contains two root-relative function locations, a
model-specific cosine score and band, `candidate_id`, `review_key`, explicit
`verification_status: "unverified"`, evidence availability, and read-only next
actions. The envelope also records the exact provider, model revision, artifact
digest, parameters, cache accounting, limits, skips, and per-phase completion.
Only `completion.status: "complete"` makes an empty candidate list conclusive
for the admitted scope.

Use that same unchanged discovery document to inspect one exact candidate before
judging it. Do not rerun discovery between inspection and review:

```bash
npx fallow similar-code inspect <candidate_id> \
  --candidates similar-code.json --format json --quiet
```

The candidate document is a bounded snapshot handoff. Inspect selects exactly
that `candidate_id` from the original envelope and does not rerun provider
retrieval or global ranking. It re-extracts only the candidate's two endpoints
from the current project root and requires both `source_sha256` values to still
match. Changed, missing, ambiguous, or out-of-root source fails closed instead
of silently inspecting a different candidate. Human output prints the complete
snapshot-based command.

Inspect reproduces the candidate against the current source and adds bounded
source windows plus deterministic context where available: graph relationship,
entry-point reachability, callers, callees, CODEOWNERS ownership, recent churn,
related tests, and overlap with deterministic clone groups. Missing evidence is
reported explicitly. Runtime evidence is reserved in the contract but is not
requested in version 1.

Write a separate verdict document:

```json
{
  "schema_version": "1",
  "verdicts": [
    {
      "candidate_id": "sc_example",
      "review_key": "scr_example",
      "candidate_worthy": true,
      "behaviorally_equivalent": false,
      "refactor_safe": false,
      "outcome": "related-but-distinct",
      "rationale": "Both normalize input, but one preserves empty values."
    }
  ]
}
```

Join it without changing the raw discovery document. `inspect` and `review`
must receive the same `similar-code.json` snapshot:

```bash
npx fallow similar-code review \
  --candidates similar-code.json \
  --verdicts verdicts.json \
  --require-verdict-for-each-candidate \
  --format json --quiet
```

The three judgment axes remain independent. `refactor_safe: true` requires
`behaviorally_equivalent: true`, which requires `candidate_worthy: true`.
Unknown judgments use `null`. The domain outcome is one of
`same-responsibility`, `related-but-distinct`, `intentional-duplication`,
`unrelated`, or `needs-human-review`.

## Agent flow

MCP exposes two read-only tools:

- `find_similar_code` returns the raw candidate envelope. Scope it with
  `paths`, `workspace`, `changed_since`, `threshold`, `min_lines`, or `top`.
- `inspect_similar_code` accepts one `candidate_id` plus a typed `snapshot`
  containing the unchanged discovery `schema_version`, `generation`, selected
  `candidate`, `completion`, and `diagnostics`, then returns its inspect packet.

For scoped discovery, `generation.scope.paths` is the sorted materialized scope
that actually reached analysis. It is provenance for auditing the run, not an
argument list to replay. Construct the MCP snapshot directly from the discovery
envelope and selected candidate. Inspect validates the snapshot endpoints
against current source without repeating global retrieval or ranking. The CLI's
candidate action renders the bounded `--candidates` command. An empty path list
with `active: false` means discovery was unscoped.

Both tools use a dedicated 15-minute subprocess window so a cold local run does
not require a generic timeout override. Operators can still set
`FALLOW_TIMEOUT_SECS` to choose a different bound.

Code Mode intentionally exposes neither tool because its maximum 30-second
execution window cannot satisfy the cold-run contract. Call the standalone MCP
tools directly; `fallow.run("find_similar_code", ...)` and the former
convenience aliases are not supported.

Neither tool downloads a model, writes verdicts, edits source, or turns a score
into a finding. If setup is missing, ask the user to run
`fallow similar-code setup --local` themselves. An agent should inspect source,
tests, callers, and side effects before it authors a verdict. It should abstain
with `needs-human-review` when the evidence is insufficient.

The Node API exposes `detectSimilarCode(options)` and a precise
`SimilarCodeReport` declaration for generation provenance, completion, skips,
diagnostics, and candidate actions. The npm loader resolves the
exact companion package, verifies its detached Ed25519 signature and embedded
digest, then supplies the trusted provider path to the Rust API. It does not
search project commands or `PATH`.

## Configuration

Only project-owned calibration belongs in config:

```json
{
  "similarCode": {
    "threshold": 0.8,
    "minLines": 3,
    "ignore": ["src/generated/**"]
  }
}
```

Provider identity, model selection, executable paths, downloads, credentials,
and consent are deliberately not configurable. `threshold` is specific to the
pinned model and remains a discovery cutoff, not a universal quality score.

Clear derived project vectors without removing the user-level model:

```bash
npx fallow similar-code cache clear --yes
```

## Trust and privacy boundary

The companion is native Rust built with Candle. It uses the pinned
`jinaai/jina-embeddings-v2-base-code` revision declared in
`crates/api/similar-code-protocol.json`. Setup verifies exact artifact sizes and
SHA-256 digests. Analysis runs with network-related environment removed and an
offline marker set. Source fragments are sent only to the local companion over
bounded stdio and are not persisted. The persistent cache contains vectors
keyed by full source digest plus model, extraction, and parameter provenance.

The CLI accepts only a sibling companion file. The Node adapter accepts only an
exact-version package path that its loader verified first. Project config,
`PATH`, project-local executables, remote providers, and provider-returned paths
are outside the trust boundary.

## Architecture and ownership

The capability follows the normal pipeline with an isolated provider boundary:

```text
config -> discovery -> function extraction -> local vector cache
       -> verified local embedding -> bounded comparison -> raw output
       -> inspect enrichment -> external verdict join
```

Ownership is split as follows:

- `crates/extract/` and `crates/engine/src/source.rs` own deterministic function
  extraction and source digests.
- `crates/engine/src/similar_code.rs` owns provider-neutral validation,
  admission, comparison, stable identities, and ranking.
- `crates/api/src/similar_code/` owns trusted companion discovery, protocol,
  vector persistence, and local transport.
- `crates/api/src/runtime/similar_code.rs` owns orchestration, scope, inspect
  enrichment, and verdict joining.
- `crates/output/src/similar_code.rs` owns independent public envelopes.
- `tools/similar-code-sidecar/` owns model setup and Candle inference.
- `npm/fallow-similar-code*` owns exact-version companion distribution.
- CLI, MCP, and NAPI remain thin adapters over the shared API.

The release workflow must publish the companion platform packages and root
package before consumers that declare the optional dependency. Main platform
packages also carry the signed sidecar beside the multicall binary, and their
verification sentinel covers both executables.

## Version 1 boundaries

Function extraction intentionally admits supported named top-level functions,
bound function and arrow declarations, and named methods on supported
top-level classes and objects. Nested callbacks, constructors, accessors,
computed methods, and other unsupported syntax stay outside version 1. Their
omission is reported through extraction skips and partial completion rather
than treated as proof that no similar code exists.

Version 1 intentionally does not run from bare `fallow`, `audit`, `dupes`,
GitHub Action gates, GitLab gates, SARIF, LSP, VS Code, or auto-fix. Those
surfaces require measured precision and a separate product decision. It also
does not accept third-party models or remote inference. Expanding either
boundary requires a versioned protocol and output-contract review.
