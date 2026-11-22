### Fixed

- **The unmatched `ignoreFindings` note no longer advises a removal that changes which paths are hidden.** The note says that a pattern has no effect and that you can remove it. That advice was wrong in two cases. When no positive pattern matched beside `!` exceptions, the removal of the positive patterns makes `ignoreFindings` hide every path outside the exceptions. When there is no positive pattern and no `!` exception matched, the removal of the exceptions makes `ignoreFindings` hide no path. Fallow no longer reports these patterns as `ignore-findings-pattern-unmatched`.
