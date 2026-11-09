### Added

- **`fallow mutation` has nine opt-in operators from the StrykerJS mutator set.** They run only when `--operators`, `mutation.operators` or the MCP `operators` parameter names them:
  - `block-statement` removes the statements of a block or function body without declarations;
  - `array-literal` changes `[x]` to `[]` and `[]` to `["fallow"]`;
  - `object-literal` changes a non-empty object literal to `{}`;
  - `method-expression` swaps method pairs such as `startsWith` and `endsWith`, and removes calls such as `.trim()` and `.filter(...)`;
  - `switch-case` removes the statements of a clause;
  - `call-expression` changes a call in an expression position to `(void 0)`;
  - `throw-statement` removes `throw new X(...)`;
  - `regex-constructor` applies the regex changes to a `new RegExp("...")` pattern;
  - `regex-assertion` negates a lookaround and swaps `\b` and `\B`.

  The default set does not change. `docs/mutation-testing.md` lists both sets.

### Fixed

- **`return-value` now mutates a concise arrow body that is a call, such as `() => save()`.** Before, such a body got no mutant.
- **A branch or operand constant now gives no equivalent mutant through `!`.** `!(items.length >= 0) || flag` on an array no longer gives `false || flag`.
- **A mutant at the start of an expression statement now keeps the grouping of the rest of the statement.** The guard against a join with the line before is now `void 0,`, not `void`. Before, `!x ? a() : b();` became `void (x) ? a() : b()` in the report and always ran `b()`, and `f() ** 2;` gave a syntax error.
