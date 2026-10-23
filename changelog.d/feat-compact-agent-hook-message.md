### Changed

- **The agent gate prints a short block message.** When the gate that
  `fallow agent install` and `fallow hooks install --target agent` write
  blocks a `git commit` or `git push`, stderr now holds one line for each
  finding that caused the block, the verdict, and the path of the full JSON
  report. Under the default `gate = "new-only"` the list holds only the
  introduced findings, so inherited findings no longer fill the context of
  the agent. The full report goes to `.fallow/gate-report.json` in the
  audited tree. Set `FALLOW_GATE_REPORT` to write it to another path, or
  `FALLOW_GATE_OUTPUT=json` to print the full report on stderr as before.
  The exit codes do not change. Run `fallow agent install` again to update an
  installed gate. Thanks @ankoor7 for the report.
  (Closes [#3725](https://github.com/fallow-rs/fallow/issues/3725))
