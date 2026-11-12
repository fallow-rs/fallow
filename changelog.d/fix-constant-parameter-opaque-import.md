### Changed

- **The opt-in `constant-parameter` rule now skips an exported function that an `import()` or a `require` call with an unknown source can call by name.** A call such as `import(name)`, `` import(`${name}`) ``, `import(flag ? './a' : name)` or `require(name)` can load any module. When the code reads `mod.pad`, `mod['pad']`, `const { pad } = mod` or `'pad' in mod` on the loaded value, the rule skips each exported function named `pad`, also through a re-export. When the loaded value goes to other code, for example as an alias, a call argument or a return value, the rule skips each exported function. In type-aware mode, the same rules apply to the methods of exported classes. A discarded value, such as `await import(name);`, skips nothing. A source that ends in a data extension, such as `` import(`@/locales/${lang}.json`) ``, does not count. An import that resolves to a file, or a pattern with a relative prefix such as `` import(`./plugins/${name}.ts`) ``, still blocks only the files that it matches. Under `--production`, such a call in a test file does not count. The cache format changes, so the first run after the upgrade parses each file again.
  (Closes [#4556](https://github.com/fallow-rs/fallow/issues/4556))

### Fixed

- **`fallow dead-code` now credits the pattern branch of a conditional `import()` source.** In `import(flag ? './a' : './plugins/' + name)`, the files that `./plugins/` matches are no longer reported as unused files. Before, only the static branch `./a` gave an edge.
