### Fixed

- **A `sh -c` script keeps the entry points of each chained command.** In a
  package.json script such as `sh -c "echo start && node tool.js"`, fallow
  found only the command before the first operator. It reported `tool.js` as
  an unused file. Fallow now reads the quoted body of `sh -c`, `bash -c`,
  `zsh -c` and `dash -c` as a command list, with `&&`, `||`, `;` and `|`.
  This is also true when `c` is in a flag cluster, as in `bash -lc`. An
  operator in quotes no longer splits a command, so a quoted argument such as
  `"x; node b.js"` does not give a false entry point.
  (Closes [#4174](https://github.com/fallow-rs/fallow/issues/4174))
