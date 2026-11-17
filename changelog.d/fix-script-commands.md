### Fixed

- **A dependency that only the inline code of `node -e` loads is now used.** A script such as `node -e "require('shelljs').ls()"` or `node --input-type=module -e "import shelljs from 'shelljs'"` now credits `shelljs`. Fallow reads the `import`, `export from`, `import()` and `require()` specifiers of the code value of `-e`, `--eval`, `-p` and `--print` for `node`, `tsx` and the other Node runtimes. A relative path and a Node built-in module credit no package. A flag after the script file goes to the script and credits nothing.
  (Closes [#4711](https://github.com/fallow-rs/fallow/issues/4711))
- **A quoted GitHub Actions `run:` value now credits the script that it runs.** A step such as `run: 'node scripts/aggregate_evals.js artifacts'` or `run: "node scripts/build.js"` no longer reports the script as an unused file. Fallow now decodes a single-quoted or double-quoted `run:` value before it reads the command: `''` in a single-quoted value is one `'`, and a backslash escape in a double-quoted value is decoded. A quoted value over several lines and a quoted GitLab CI `script:` item are decoded too.
  (Closes [#4727](https://github.com/fallow-rs/fallow/issues/4727))
