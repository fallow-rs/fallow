### Fixed

- **`fallow audit` no longer reports a `skip-only` weakened test for a skip marker that the base already had.** A change that edits other text on an `it.skip(` or `test.only(` line, for example a callback that becomes `async` or a new test title, gave the finding "The change adds `it.skip`". Fallow now compares skip and focus markers by chain and title, not by line text. A new marker next to an existing one, or a skip that moves to another test, still gives a finding.
