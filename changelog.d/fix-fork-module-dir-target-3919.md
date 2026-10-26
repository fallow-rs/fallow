### Fixed

- **`fallow dead-code` follows `child_process.fork` targets built from the module directory.** A file that `fork(path.join(path.dirname(fileURLToPath(import.meta.url)), "child.ts"))` starts is no longer an unused file. Fallow also reads `path.resolve`, `__dirname`, `import.meta.dirname` and a constant that holds the module directory. A target with a computed segment stays unresolved. Thanks [@VariableVince](https://github.com/VariableVince) for the report. (Closes [#3919](https://github.com/fallow-rs/fallow/issues/3919))
