### Fixed

- **`fallow init` keeps a `.gitignore` that is not UTF-8.** It appends
  `.fallow/` to the bytes of the file. Before, it replaced such a file with
  the one line `.fallow/`.
- **A partial save of a regression baseline keeps a file that it cannot use.**
  `fallow flags --retirement --save-regression-baseline <PATH>` and
  `fallow list --entry-weight --save-regression-baseline <PATH>` stop with exit
  code 2 when the file exists and does not parse, or has a `schema_version`
  that this build does not read. The file stays unchanged. Before, the save
  wrote a new file without the other blocks.
- **`fallow dead-code --save-regression-baseline <PATH>` warns when it replaces
  a file that was not usable.** The warning names the entry weights and flag
  counts that the new file does not keep.
- **`fallow audit --walkthrough --mark-viewed <PATH>` keeps a ledger that it
  cannot use.** It prints a warning and does not change a ledger that it
  cannot read, that does not parse, or that a newer fallow wrote.
- **`fallow agent status` names an MCP settings file that does not parse.**
  The MCP row says that the file does not parse, and no longer says
  "no fallow entry".
- **`fallow cloud login` and `fallow cloud logout` keep a credentials file of a
  newer version.** They stop with an error that names the file, so the change
  cannot drop the fields that a newer fallow wrote.
