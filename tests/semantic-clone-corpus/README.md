# Semantic clone conformance

This corpus separates three questions that token matching and vector similarity
must not collapse:

- `candidate_worthy`: should a reviewer inspect the pair?
- `behaviorally_equivalent`: do the implementations produce the same behavior?
- `refactor_safe`: is an actionable shared-code refactor supported by the
  fixture evidence?

Current `dupes`, `--mode semantic`, and `--near` output remains actionable
duplication evidence. Model similarity is evaluated separately as advisory
candidate evidence.

## Deterministic baseline

Build Fallow, verify every pinned fixture digest, and evaluate whole-pair
coverage:

```bash
cargo build -p fallow-cli --bin fallow
npm run --silent conformance:semantic-clones -- --pretty --check
```

The baseline uses `semantic` normalization plus the opt-in near pass. A pair is
detected only when one clone group covers the configured share of both files.
Small shared fragments therefore do not turn an unrelated pair into a hit.

## Recorded local-model evidence

`evidence/jina-v2-code-q8.json` records a local ONNX experiment using the
Apache-2.0 licensed `jinaai/jina-embeddings-v2-base-code` model at an immutable
revision. Source code remained on the machine. The model runtime, artifact,
dimensions, threshold, parameters, runtime observation, and per-case
similarities are locked in the evidence file and its manifest digest.
Batch size is part of the generation identity because provider output can vary
with padding and batching behavior.

At the recorded threshold, the native 768-dimensional profile adds the
helper-extraction candidate missed by deterministic matching and retains the
renamed-identifier candidate. It does not recover the different-algorithm pair.
No adversarial negative pair crosses the threshold in this small corpus.

The recorded 256-dimensional profile is a naive truncation experiment. This
model was not trained to expose 256-dimensional Matryoshka output, so the result
must not be used as evidence that 256 dimensions are generally safe. A suitable
model must declare support for the requested output width.

The observed post-embedding process RSS was high enough that this model should
not be linked into Fallow's core binary. Provider execution remains an
orchestration concern, while candidate evaluation stays pure and bounded.

## Reproduce model evidence

Install the optional model runtime outside the repository from the committed
package lock, then pass its module path explicitly. The generator rejects a
different lock or a module outside that locked installation. The command
downloads the pinned model into the runtime cache, but inference is local and
source is not uploaded.

```bash
embedding_env=$(mktemp -d /tmp/fallow-embedding-evidence.XXXXXX)
cp tests/semantic-clone-corpus/runtime/package.json "$embedding_env/package.json"
cp tests/semantic-clone-corpus/runtime/package-lock.json "$embedding_env/package-lock.json"
(cd "$embedding_env" && npm ci)
node scripts/semantic-clone-model-evidence.mjs \
  --runtime-lock "$embedding_env/package-lock.json" \
  --transformers-module \
  "$embedding_env/node_modules/@huggingface/transformers/dist/transformers.node.mjs" \
  --model-cache-state unknown
```

Runtime and memory observations can vary by machine. Similarities are rounded
and must be reviewed before replacing the locked evidence.

To review a runtime update, do these steps:

1. Run the generator with the old lock and add
   `--compare tests/semantic-clone-corpus/evidence/jina-v2-code-q8.json`. All
   deltas must be zero. This proves that the machine gives the locked numbers.
2. Run the generator with the new lock and the same `--compare` flag. Review
   the deltas, the threshold crossings, and the case order on stderr.

The `--compare` flag writes the review summary to stderr only. The JSON evidence
on stdout does not change.

## Provenance

The source fixtures are unmodified excerpts from
[`rafal-qa/embedding-benchmark`](https://github.com/rafal-qa/embedding-benchmark)
at the commit recorded in `manifest.json`. Each file has an upstream path,
SHA-256 digest, and license entry. The retained upstream license and detailed
attribution are in `LICENSE.embedding-benchmark` and `NOTICE.md`.
