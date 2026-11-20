# Output assembly outside `crates/output`

`fallow-output` owns the typed output contracts: the envelope types, the
schema, and the shared format builders. Some output is still built in other
crates. This page lists each such site, what it builds, and the verdict for
it. Read it before you change a JSON, SARIF, CodeClimate, Markdown, compact
or editor output, so that you change the site that builds it.

A verdict is one of these:

- **stays**: the site owns a fact that only its surface has.
- **move**: the builder belongs in `fallow-output`. An issue tracks a move
  with a known need. A move without a known need waits until a change
  touches the site.
- **contract decision**: the move changes output that a user can see, so the
  maintainer decides the shape first.

The line numbers are the entry points at the time of writing. Search for the
function name when a line moved.

## JSON envelopes in `fallow-api`

| Site | What it builds | Verdict |
|---|---|---|
| `crates/api/src/json_output.rs:231` (`serialize_check_json`), `:343` (`serialize_duplication_json`) | the dead-code and duplication JSON documents | move, one envelope per change; no known need |
| `crates/api/src/combined_output.rs:82` (`serialize_combined_json`) | the bare-run JSON document | move; no known need |
| `crates/api/src/runtime_json.rs:38` (`serialize_combined_programmatic_json`), `:156` (`serialize_audit_programmatic_json`) | the JSON of the programmatic runs | move; no known need |
| `crates/api/src/audit_output.rs:228` (`build_audit_header_json`) | the audit JSON header | move; no known need |
| `crates/api/src/list_output.rs:43` (`build_list_json_output`) | the `fallow list` JSON | move; no known need |
| `crates/api/src/grouped_output.rs:78` (`group_analysis_results_with`) | the groups of `--group-by` | move with the envelopes that use it |

The JSON envelope types are in `fallow-output` already
(`crates/output/src/root_envelopes.rs`). The functions above fill them, so the
JSON contract has one schema. A move puts the fill code next to the types.

## Text and CodeClimate formats in `fallow-api`

| Site | What it builds | Verdict |
|---|---|---|
| `crates/api/src/markdown_output.rs:1088` (`build_grouped_markdown`), `:1329`, `:1540`, `:1605` | Markdown | move after the JSON envelopes; no known need |
| `crates/api/src/compact_output.rs:205` (`build_compact_lines`), `:745`, `:758`, `:803` | the compact lines | move after the JSON envelopes; no known need |
| `crates/api/src/dead_code_codeclimate.rs:1459` (`build_codeclimate`) and its `dead_code_codeclimate_*.rs` parts | dead-code CodeClimate | move after the JSON envelopes; no known need |
| `crates/api/src/health_codeclimate.rs:213` (`build_health_codeclimate`) | health CodeClimate | move after the JSON envelopes; no known need |
| `crates/api/src/dupes_output.rs:448` (`build_duplication_codeclimate`) | duplication CodeClimate | move after the JSON envelopes; no known need |

## Saved-envelope renderers of `fallow report --from`

| Site | What it builds | Verdict |
|---|---|---|
| `crates/cli/src/report/sarif.rs:225` (`print_envelope_sarif_with_config`) | SARIF from a saved envelope | stays in the CLI |
| `crates/cli/src/report/codeclimate.rs:65` (`print_envelope_codeclimate_with_config`) | CodeClimate from a saved envelope | stays in the CLI |
| `crates/cli/src/report/markdown.rs:114` (`print_saved_markdown`) | Markdown from a saved envelope | stays in the CLI |

`fallow report` is a CLI command. These renderers read a JSON file, not an
analysis result, and only the CLI has that input.

## Sentences from envelope members

| Site | What it builds | Verdict |
|---|---|---|
| `crates/output/src/verdict_facts.rs` (`read_gate_verdicts`, `read_request_verdicts`, `read_baseline_verdict`, `EmptyScope`, `gate_number_text`) | the facts of a verdict: which gates an envelope reports, the enforced default, the gate number text, the requests that applied over an empty scope, and the facts of each loaded baseline | owned by `fallow-output` |
| `crates/cli/src/report/gate_outcome_text.rs` (`summary_line`, `gate_rows`, `exit_reason_line`) | the CLI gate lines from `gate_outcomes` | sentences stay in the CLI; facts from `verdict_facts` |
| `crates/cli/src/report/request_outcome_text.rs` (`summary_line`) | the CLI request lines from `request_outcomes` | sentences stay in the CLI; facts from `verdict_facts` |
| `crates/cli/src/report/baseline_advisory_text.rs` (`advisory_line`) | the stale baseline advisory from `baseline_staleness` | sentences stay in the CLI; facts from `verdict_facts` |
| `crates/mcp/src/tools/gate_verdicts.rs` (`annotate_gate_verdicts`) | the verdicts of a CLI-backed MCP tool | sentences stay in the MCP server; facts from `verdict_facts` |

The CLI and the MCP server write different sentences on purpose. For example,
the CLI baseline advisory uses Markdown and a remedy without a channel, and
the MCP sentence names the `save_baseline` parameter. So each surface keeps
its sentences. The facts under the sentences must agree, so both surfaces read
them from `fallow-output`:

- A gate number shows two decimals at most, and a whole number shows no
  fraction. The mutation score shows one decimal, rounded down. An observed
  number that differs from its threshold never shows as equal to it: the
  text then shows more decimals, for example `5.004` against `5`.
- A gate entry without `enforced` reports as not enforced.
- A baseline reads `warning`, `gate_trips`, the entry counts,
  `change_scoped`, `unrecognised_format` and `scope_reasons` with one set of
  defaults. A `saved_by` value that is not a kebab-case command name reads as
  absent.
- Only a change scope (`changed-since`, `diff-filter`) that applied over an
  empty scope gets the advice to check the diff or the base ref.

## Surface-owned members and text

| Site | What it builds | Verdict |
|---|---|---|
| `crates/mcp/src/tools/mod.rs:1173` (`ensure_top_level_warnings`) | the root `warnings` member of an MCP tool result | stays MCP-owned |
| `crates/cli/src/report/json.rs:143` (`check_output_meta`) | the `_meta` member of the CLI JSON | stays; it picks a builder of `fallow-output` (`check_meta`, `architecture_meta`), and the programmatic JSON has `_meta` too |
| `crates/lsp/src/diagnostics/unused.rs`, `structural.rs`, `quality.rs` and the other files in `crates/lsp/src/diagnostics/` | the message text of each editor diagnostic | a shared message source later; no known need |
| `action/scripts/analyze.sh:669` (`BASELINE_STALENESS_JQ`) | the baseline advisory of the GitHub Action, read with `jq` | out of scope: the Action reads the envelope with shell tools |
