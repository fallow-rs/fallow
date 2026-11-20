### Changed

- **`fallow <command> --help` groups the options under headings.**
  Each global option has a heading: Project, Output, Scope, Baselines,
  Gates, Duplication, Type-aware or Performance. Each command with more than
  six options of its own groups them under two to five headings, for example
  Issue types, Tracing, Scope and Output for `dead-code`, and Scope,
  Baselines, Coverage, Gates and Review for `audit`. A command heading and a
  global heading with the same name print as one section. The `help_heading`
  field of `fallow schema --cli-reference` carries each heading. In the
  default `fallow schema` output, `--top` of `dead-code`, `health` and
  `flags`, `--sort` of `health`, and the global `--no-cache`, `--threads`
  and `--performance` flags move, so that each list follows the order of
  `--help`. The root `fallow --help` keeps one options block.
