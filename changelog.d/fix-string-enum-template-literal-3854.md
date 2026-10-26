### Fixed

- `fallow security` now resolves a string enum member in the same module like a `const` string. A template literal such as `` `<meta name="${MetaName.BUILD_NUMBER}">` `` that goes to an HTML sink is no longer reported as non-literal HTML. Enums and constants that you import from another module stay unresolved, because sink extraction reads one module at a time. The extraction cache version changes, so the first run after the update parses all files again. Thanks [@Smrtnyk](https://github.com/Smrtnyk) for the report. (Closes [#3854](https://github.com/fallow-rs/fallow/issues/3854))
