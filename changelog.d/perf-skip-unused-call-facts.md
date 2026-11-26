### Performance

- **Extraction skips the call facts when no rule reads them.** Only the `constant-parameter` rule family and the `health` command read the call facts of a file. Other runs now skip the pass that collects them, so a cold `fallow dead-code` run does less work per file. The output does not change.
