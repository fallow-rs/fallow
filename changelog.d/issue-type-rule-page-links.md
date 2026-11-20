### Changed

- **The docs link of each issue type opens its rule page.** `fallow explain`,
  the `_meta.rules` docs of `--explain`, the SARIF `helpUri`, the LSP code
  description, the MCP `fallow://issue-types` resource, `fallow schema`, the
  section footers of the human report and the rows of the GitHub summary now
  link to `https://fallow.tools/docs/rules/<id>/`. The rule page has the
  facts of the rule, an example, the fix and the suppression comment. The
  old headings on the explanation pages stay, so the links of older versions
  still work.
  (Closes [#4894](https://github.com/fallow-rs/fallow/issues/4894))
