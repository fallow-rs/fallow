### Fixed

- **`fallow mutation --recheck` now keeps the earlier results and replaces the `--report` file.** Before, a recheck wrote its report only to `.fallow/mutation/last-report.json`. So `fallow mutation tasks --report <file>` still listed each old task. Also, each earlier kill became `pruned` with `prune_reason: "not-selected"`, so the summary did not describe the full set.

  Now a recheck without `--mutant` replaces the `--report` file too. Each mutant that the recheck did not select keeps its earlier status and evidence, with `carried: true`. The summary and the score count the merged set. An earlier result applies only while its file has the digest of the earlier report. An earlier kill also needs the test files of its killing tests. When a file loses its earlier results, `recheck.stale_files` names it.

  The `recheck` object adds the count `carried`. The kill matrix stays the evidence of the run. The recheck does not run a carried mutant again. After you remove or change a test, run `fallow mutation` without `--recheck`.
  (Closes [#4477](https://github.com/fallow-rs/fallow/issues/4477))
