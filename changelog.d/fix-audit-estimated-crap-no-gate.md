### Changed

- **`fallow audit --gate new-only` no longer fails on a CRAP score from
  estimated coverage alone.** Without a coverage map, Fallow estimates CRAP
  from static test reachability. This estimate does not measure the change. A
  complexity finding that exceeds only the CRAP threshold with estimated
  coverage now has `effective_severity: warn`. It stays in the report, and the
  verdict is at least `warn`, with exit code 0. SARIF reports it at level
  `warning`.

  A finding that also exceeds the cyclomatic or cognitive threshold still
  gates. CRAP from measured coverage (`--coverage`, `health.coverage`,
  `FALLOW_COVERAGE` or runtime coverage) still gates. `--gate all` does not
  change.

### Fixed

- **`fallow audit --gate new-only` no longer reads a missing base CRAP score as
  growth.** A matched complexity finding was introduced when the head had a
  CRAP score and the base finding had none. The two runs then had different
  coverage input, and the code did not grow. Such a finding is now `worsened`
  or `inherited`. A CRAP score from estimated coverage also no longer makes a
  matched finding introduced; it can make it `worsened`.
