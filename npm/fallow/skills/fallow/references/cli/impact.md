# `impact`: Local Impact History

Read the opt-in Impact history stored in the user's private config directory.
These commands start no analysis and never upload data.

```bash
fallow impact
fallow impact status
fallow impact statusline
fallow impact --all --sort recent
```

`fallow impact statusline` is the stable status-surface command. It always
prints exactly one plain-text, path-free line, ignores the global output format,
and performs no migration write. Whole-project counts come from the last full
`fallow` scan and their trend compares only the prior full scan. Older stores
with changed-file history label that narrower scope explicitly and omit its
non-comparable trend.

The command does not enable tracking. Only the user may opt in with
`fallow impact enable` or `fallow impact default on`.
