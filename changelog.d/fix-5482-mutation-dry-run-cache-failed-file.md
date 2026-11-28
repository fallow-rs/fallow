### Fixed

- **`fallow mutation` keeps the dry-run cache when one test file has a failed baseline test.** A test file with a failed test gets no stored dry-run facts, so it dry-runs in each run. Before this fix, the other test files of its earlier dry run then lost their stored facts too, and each later run did the full dry run again. A stored file now keeps the keys of the test files that can give its share facts, and dry-runs again only when one of those keys changes. The `cache` object of the JSON report adds `dry_run_miss_reasons`, the misses for each reason. The dry-run cache file has a new format version, so the first run after the update dry-runs each test file.
  (Closes [#5482](https://github.com/fallow-rs/fallow/issues/5482))
