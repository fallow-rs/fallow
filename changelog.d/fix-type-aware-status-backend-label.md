### Fixed

- **`fallow type-aware status` names the tsrs backend in its human output.** With `FALLOW_TYPE_AWARE_BACKEND=tsrs`, the line said `TypeScript 0.20.1-ts7.1.0-beta`, but that version is the tsrs version. The line now says `tsrs 0.20.1-ts7.1.0-beta`. The line for the default backend and the JSON output do not change. The `_meta` description of the Svelte virtual-module abstention count now says "the type checker", because the text also applies to a tsrs run. Thanks @Smrtnyk for the report. (Closes [#4066](https://github.com/fallow-rs/fallow/issues/4066))
