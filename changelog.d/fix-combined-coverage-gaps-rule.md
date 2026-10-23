### Fixed

- **Bare `fallow` now applies the `coverage-gaps` rule, as `fallow health` does.** Before, the combined run did not compute coverage gaps, so a `coverage-gaps` rule at `error` never failed it. Now an enabled rule shows the gaps in the health section, and a rule at `error` makes the human run exit 1. The JSON `gate_outcomes` object gets a `health-coverage-gaps` entry. Like the other findings rules of the combined run, the machine formats exit 1 on it only with `--fail-on-issues` or `--ci`. Thanks to @Smrtnyk for the report (#3823).
