### Fixed

- **The weekly fuzz smoke run completes inside its time limit.** The scheduled run fuzzes each target for 300 seconds, one after the other, so it needs about 67 minutes. The job limit for a scheduled run is now 90 minutes. A push run keeps the 45-minute limit. A policy test checks that the job limit covers the fuzz time of all targets for each event. This change affects contributors only.
  (Closes [#4619](https://github.com/fallow-rs/fallow/issues/4619))
