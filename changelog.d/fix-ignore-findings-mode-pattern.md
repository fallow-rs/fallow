### Fixed

- **The unmatched `ignoreFindings` note no longer advises the removal of a pattern that sets the mode.** The note says that a pattern has no effect and that you can remove it. That is false for two patterns. The first is the only positive pattern beside `!` exceptions: without it, `ignoreFindings` hides every path outside the exceptions. The second is the only `!` exception without a positive pattern: without it, `ignoreFindings` hides no path. Fallow no longer reports these two patterns as `ignore-findings-pattern-unmatched`.
