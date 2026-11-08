### Added

- **`fallow mutation --time-budget` now runs the cheapest mutants first.** With a time budget, the first pass starts the mutants with the lowest estimated test time first. The estimate is the sum of the dry-run times of the covering tests. The budget now settles more mutants. Without a budget, the order does not change.
