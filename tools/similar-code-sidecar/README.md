# fallow-similar-code

First-party local embedding sidecar for Fallow's advisory similar-code
capability. It uses Candle's native Rust JinaBERT implementation and does not
embed a JavaScript model runtime.

The protocol and model provenance come from
`crates/api/similar-code-protocol.json`. Wire protocol v2 and embedding
semantics v2 pin
`jinaai/jina-embeddings-v2-base-code` revision
`516f4baf13dec4ddddda8631e019b5737c8bc250`, 768 dimensions, mean pooling,
L2 normalization, and Apache-2.0 licensing.

## Commands

```bash
# JSON readiness and provenance
fallow-similar-code status --json

# Explicit local setup. Downloads and SHA-256 verifies only the three pinned
# model artifacts into the user cache.
fallow-similar-code setup --local --json

# JSONL stdin/stdout session, with no network access during analysis
fallow-similar-code serve
```

An `embed-functions` request is one JSON line:

```json
{
  "operation": "embed-functions",
  "protocol_version": 2,
  "embedding_semantics_version": 2,
  "model_revision": "516f4baf13dec4ddddda8631e019b5737c8bc250",
  "dimensions": 768,
  "max_tokens": 1024,
  "functions": [{ "key": 0, "source": "export const add = (a, b) => a + b;" }]
}
```

Responses retain the required protocol fields and add typed completion and
error information when a source, function, batch, or time budget limits work.
Functions longer than the pinned 1024-token model budget are truncated and
marked in the vector and completion metadata. Pooling is attention-mask-aware.
Batch size is deliberately fixed at one to bound peak memory.

Inference runs in F32 on the CPU. On macOS, every build links the Accelerate
framework through the `accelerate` feature of `candle-core`, set as a
macOS-only dependency in `Cargo.toml`. Linux and Windows builds use the
default Candle CPU path. A change to this build setup changes the vectors, so
it needs a bump of `embedding_semantics_version`.

## Privacy and cache

- Setup is the only command that uses the network.
- Setup bounds DNS, connect/TLS, response-header, response-body, and complete
  per-artifact phases independently. The npm executable also bounds the full
  direct setup process.
- Serve reads source only from stdin. It never writes source to disk, logs it,
  or includes it in errors.
- Only verified vectors live in the parent Fallow process cache. This sidecar
  owns model artifacts, not a source or vector cache.
- Set `FALLOW_SIMILAR_CODE_CACHE_DIR` to an absolute directory to override the
  user cache location, including in hermetic tests.
- Every serve process verifies artifact sizes and SHA-256 digests before
  loading the model.

## Development

```bash
cargo test --manifest-path tools/similar-code-sidecar/Cargo.toml
cargo clippy --manifest-path tools/similar-code-sidecar/Cargo.toml --all-targets -- -D warnings
cargo fmt --manifest-path tools/similar-code-sidecar/Cargo.toml -- --check
npm run check:similar-code-sidecar-audit
```

Model weights are not needed for protocol, status, setup-integrity, or failure
tests. The release workflow downloads the exact signed Linux x64 artifact,
performs explicit local setup, and compares real F32 Candle similarities with
the compact committed baseline:

```bash
npm run conformance:semantic-clones:candle -- \
  --sidecar-bin tools/similar-code-sidecar/target/release/fallow-similar-code
```

The baseline contains only model provenance, similarities, and selection
decisions. It contains no fixture source or embedding vectors.

The temporary `audit-allowlist.json` owns only these advisories and expires
automatically:

- `RUSTSEC-2024-0436` (`paste` 1.0.15 is unmaintained). Remove it as soon as
  released Candle, gemm, and tokenizers versions remove both remaining `paste`
  dependency chains.
- `RUSTSEC-2026-0367`, `RUSTSEC-2026-0368`, and `RUSTSEC-2026-0369` (`zip`
  8.6.0 extraction flaws). Candle 0.11.0 requires `zip` 8.6.0 and only reads
  archive entries. The sidecar loads only safetensors, so it never extracts a
  ZIP archive. Remove them as soon as a released `candle-core` depends on
  `zip` 9.0.0 or later.

`scripts/check-similar-code-sidecar-audit.mjs` checks each entry and its
reverse dependency edges. The `cargo audit` step in the release validation
workflow ignores exactly these advisory IDs. That workflow runs before the
release dispatch and again inside the release workflow. After you remove an
entry, rerun conformance, the platform build matrix, the audit, license
checks, and the release binary size gate.
