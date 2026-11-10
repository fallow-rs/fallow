### Fixed

- **`fallow security --gate new` no longer says that it reports on the whole project when it cannot resolve `--changed-since`.** The gate needs the diff of the ref. When git cannot resolve the ref, the run now prints only the gate error and exits 2. Before, the run also printed the `--changed-since` fallback warning, which said that the report covered the whole project, but the run produced no report. The gate also resolves the ref before the analysis, so the run stops sooner. Without `--gate`, the run still reports on the whole project, warns, and records `request_outcomes["changed-since"]` as `not-applied`.
  (Closes [#4431](https://github.com/fallow-rs/fallow/issues/4431))
