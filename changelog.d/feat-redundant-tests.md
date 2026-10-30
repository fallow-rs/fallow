### Added

- Add `fallow mutation --kill-matrix` to report redundant test candidates from complete observed kill sets. Review candidates separately before removal. The opt-in mode runs all covering tests with bail disabled and can increase execution time.
- The `mutation` and `mutation-inspect` JSON envelopes move to schema version `"2"`. The mutation journal and result cache formats also change: `--resume` refuses a journal from an older release, and cached mutant outcomes run again once.
