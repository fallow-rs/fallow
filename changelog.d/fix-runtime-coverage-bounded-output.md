### Fixed

- **`fallow health --top` now cuts the large-functions list.** Before, `--top` cut the findings, file scores, hotspots and targets, but every large function stayed in the report. On a large project, a bounded MCP `check_runtime_coverage` call with `top: 20` was still more than 250,000 characters, and the MCP client saved it to a file instead of showing it. Now the same call is about 140,000 characters. The report and each `--group-by` group carry the new optional count `large_functions_omitted` when `--top` cut a row. The human output shows the full total.
