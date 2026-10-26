### Fixed

- **A CommonJS `postcss.config.cjs` no longer reports its `module.exports` keys as unused exports.** Since 3.35.0, Fallow credited only the `default` export of a PostCSS config. A tool that loads a CommonJS file reads `module.exports` as its default export, so a `default` credit from a plugin now covers every key of that object. The same rule applies to the CommonJS config files of other plugins that credit `default`. Thanks [@waldoluisribeiro](https://github.com/waldoluisribeiro) for the report. (Closes [#3876](https://github.com/fallow-rs/fallow/issues/3876))
