### Added

- **Mutation JSON now shows the edit of each mutant.** Each mutant in the
  `mutation` report and the `mutation-plan` document has a `change` object,
  for example `{"from": "??", "to": "&&"}`. The values come from a fixed
  vocabulary of operator tokens, literals and placeholders such as
  `<expression>` and `<string>`, so the report still holds no source text.
  ([#3658](https://github.com/fallow-rs/fallow/issues/3658))
