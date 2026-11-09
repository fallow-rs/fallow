### Added

- **`fallow mutation export` writes a saved report in the open `mutation-testing-report-schema` format.** Mutation dashboards and HTML report viewers read this format, so a team can keep its dashboard. The command runs no code. It maps each Fallow status to a schema status: `flaky`, `pruned` and `equivalent` become `Ignored` with a reason, `pending` stays `Pending`, and a survivor of a partial run gets a reason that says the result is not conclusive. Columns count UTF-16 code units. The export refuses a report whose mutated files changed after the run.

### Fixed

- **`fallow mutation inspect` and `fallow mutation tasks` show the code of a mutant in a file with a UTF-8 byte order mark.** The run instruments a file without its byte order mark, but `inspect` and `tasks` instrumented the file with the mark. Thus each mutant of such a file was `stale` with `mutant-not-reproduced`. They now drop the mark first.
