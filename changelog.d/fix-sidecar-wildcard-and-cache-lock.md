### Fixed

- **Two `fallow similar-code setup --local` runs on one model cache no longer break each other.** Both runs wrote the same partial, manifest and backup files, so one run removed the download of the other, and that run failed. Setup now holds a lock on the cache folder of the model revision. The second run waits, then finds the installed model and downloads nothing.
- **The type-aware sidecar reads wildcard package exports as Node does.** When fallow sent no entry points, the sidecar read the entry points from `package.json`. A target such as `./src/*.ts` then matched only files directly in `src`, and a target with two `*` accepted two different values. Each `*` of a target now stands for the same value, and that value can contain `/`, as in the fallow core.
