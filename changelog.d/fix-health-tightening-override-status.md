### Fixed

- **A `thresholdOverrides` entry that sets a lower limit no longer reads `stale` while it applies.**
  An entry can set a `maxFileLines`, `maxCyclomatic`, `maxCognitive` or
  `maxUnitSize` limit that is lower than the global limit, or a `maxFileLines`
  limit where the global limit is off. When the matched code breaches that
  lower limit, the row now reads `active`, and `outstanding` names the
  breach. Before, the row read `stale`, and that told you to remove an entry
  that produced the finding. A row reads `stale` only when the entry changes
  no result.
  (Closes [#4173](https://github.com/fallow-rs/fallow/issues/4173))
