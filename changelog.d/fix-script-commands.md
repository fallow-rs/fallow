### Fixed

- **A dependency that only the inline code of `node -e` loads is now used.** A script such as `node -e "require('shelljs').ls()"` or `node --input-type=module -e "import shelljs from 'shelljs'"` now credits `shelljs`. Fallow reads the `import`, `export from`, `import()` and `require()` specifiers of the code value of `-e`, `--eval`, `-p` and `--print` for `node`, `tsx` and the other Node runtimes. A relative path and a Node built-in module credit no package. A flag after the script file goes to the script and credits nothing.
  (Closes [#4711](https://github.com/fallow-rs/fallow/issues/4711))
