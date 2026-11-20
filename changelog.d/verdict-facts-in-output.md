### Fixed

- **The CLI gate line and the MCP `warnings` sentence now show the same gate numbers, gates, empty-scope advice and baseline facts.** Both surfaces now read the facts of the verdict from one reader in `fallow-output` (Closes [#4874](https://github.com/fallow-rs/fallow/issues/4874)). These differences are gone:

  - The CLI gate line (job summary, pull-request comment, merge-request note, GitHub annotation, the `observed` text of the decision-surface rows and the exit-reason line) now rounds a fractional gate number to two decimals, as the MCP sentence did. A duplication of `66.66666666666666` percent now reads `66.67`. The `gate_outcomes` member keeps every digit.
  - An observed number that differs from its threshold never reads as equal to it on either surface. A duplication of `5.004` percent against a threshold of `5` reads `5.004 against 5`, not `5 against 5`.
  - The MCP sentence now shows the mutation score with one decimal, rounded down, as the CLI line did. A score of `79.98` against a minimum of `80` now reads `79.9`.
  - The CLI now reports a `gate_outcomes` entry without `enforced` as not enforced, as the MCP server did. Before, the CLI dropped the entry.
  - The MCP sentence for a request that applied over an empty scope gives the advice to check the diff or the ref only for a change scope (`changed-since`, `diff-filter`). For another request, such as `group-filter`, it tells the caller to check the value of the request, as the CLI line names that request.
  - The MCP server no longer lists a `request_outcomes` entry without a string `status` as an unapplied request. The schema requires `status`, so both surfaces now skip such an entry, as they do for a gate entry.
