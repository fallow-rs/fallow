### Fixed

- **`fallow coverage upload-inventory` now names a function that is assigned to a declared variable.** For `let startLoad; startLoad = () => {}`, the inventory named the function `(anonymous_N)`. `oxc-coverage-instrument` 0.16.0 and later name it after the variable (`startLoad`), so the runtime function and the inventory function did not join in Fallow Cloud. The inventory now follows the same rule as the instrumenter for an assigned function:
  - Only a plain `=` gives a name. A compound or logical operator (`+=`, `||=`, `&&=`, `??=`) gives none.
  - Parentheses around the value do not matter: `x = (() => {})` gives `x`.
  - An assigned class does not give the name to its first field arrow.
  - A function inside the target, as in `f(() => 1).x = () => 2`, does not take the target name.

- **`fallow coverage upload-inventory` now ranks the own name of a function above the name of its binding, as `oxc-coverage-instrument` and `Function.prototype.name` do.** The inventory gave `f` for `const f = function g() {}` and `k` for `{ k: function own() {} }`. The instrumenter gives `g` and `own`, so these functions did not join in Fallow Cloud. The inventory now uses one rule for a declarator, an object property and an assignment:
  - A function with its own name keeps it: `const f = function g() {}` gives `g`, `{ k: function own() {} }` gives `own` and `x = function own() {}` gives `own`.
  - Parentheses around the value do not matter: `const f = (() => {})` gives `f` and `{ k: (() => {}) }` gives `k`.
  - A class value takes no binding name: `const C = class D {}` names only its constructor and methods.

  The stable id of a renamed function changes, because the name is one of its inputs. `fallow coverage analyze` matches an unused export to a function by name or by line. A function with its own name that starts on the line after its export (`export const f =` and then `function g() {}`) now matches neither, so a never-called function of this shape gets `review_required` and not `safe_to_delete`.

- **`fallow coverage upload-inventory` now gives a callee name only to a function that is a direct argument of the call, as `oxc-coverage-instrument` does.** The inventory also named a function deeper in an argument after the callee: `foo([() => 1])` gave `foo`, and so did a JSX attribute, an object property or a `??` operand inside an argument. When an argument function took its own or its parent name, the callee name went to the next unnamed function inside it: `foo(x = () => () => 2)` gave `foo` for the inner arrow. These functions now stay `(anonymous_N)`, as in the instrumenter. An optional call on a callee that is not a member access also gives no callee name: `foo?.(() => 1)` gives `(anonymous_N)`, and `a.b?.(() => 1)` still gives `b`.

- **`fallow coverage upload-inventory` now names more functions after their binding, as `oxc-coverage-instrument` does.**
  - A default value takes the name of its binding: `function f(cb = () => 1)` gives `cb`, `function Button({ onClick = () => {} })` gives `onClick` and `const { k: v = () => 1 } = o` gives `v`.
  - A parenthesized anonymous default export takes `default`: `export default (() => 1)`.
