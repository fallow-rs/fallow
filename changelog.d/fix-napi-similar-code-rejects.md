### Fixed

- **`detectSimilarCode` of `@fallow-cli/fallow-node` rejects its promise when the similar-code companion is not ready.**
  The function threw before it returned a promise, so a `.catch` handler or a `promise.then(ok, fail)` call never saw the `FALLOW_SIMILAR_CODE_PROVIDER_NOT_READY` error. The function now returns a rejected promise with the same error, as its `Promise<SimilarCodeReport>` type states. `await` in a `try` block works as before.
