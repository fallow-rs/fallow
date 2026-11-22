### Fixed

- **The functions of `@fallow-cli/fallow-node` reject their promise instead of throwing.**
  An invalid option, for example `minOccurrences: 1` for `detectDuplication`, and a similar-code companion that is not ready made the call throw before it returned a promise. A `.catch` handler or a `promise.then(ok, fail)` call then never saw the error. Each function now returns a rejected promise with the same error, as its `Promise` type states. `await` in a `try` block works as before. Code that caught the old throw without `await` must now handle the rejection.
