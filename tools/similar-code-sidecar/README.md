# fallow-similar-code

First-party local embedding sidecar for Fallow's advisory similar-code
capability. It uses Candle's native Rust JinaBERT implementation and does not
embed a JavaScript model runtime.

The protocol and model provenance come from
`crates/api/similar-code-protocol.json`. Wire protocol v2 and embedding
semantics v1 pin
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
  "embedding_semantics_version": 1,
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
decisions. It contains no fixture source or embedding vectors. The temporary
`audit-allowlist.json` owns only `RUSTSEC-2024-0436` and expires automatically.
Remove it as soon as released Candle, gemm, and tokenizers versions remove both
remaining `paste` dependency chains, then rerun conformance, the platform build
matrix, the audit, license checks, and the release binary size gate.
