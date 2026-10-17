### Fixed

- **`fallow health --type-aware --type-coupling` no longer fails `--type-aware-require complete` on projects with more than 40 coupling edges.** The coupling report is one report for the whole project. It now carries up to 10,000 evidence items, so a complete analysis is reported as `complete`. Symbol, impact, and API-surface results keep the limit of 40 items. Above the new limit, the report stays `partial` with the `evidence-limit` reason.
  (Closes [#3563](https://github.com/fallow-rs/fallow/issues/3563))
