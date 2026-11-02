### Added

- **`fallow security` reports `<script>` element and `createContextualFragment` sinks.** An assignment to `src`, `text`, `textContent` or `innerText` on a local bound to `document.createElement("script")` is now a `code-injection` candidate, with the callee `HTMLScriptElement.<property>`. The same property on another element stays quiet. A non-literal argument to `Range.createContextualFragment()` is now a `dangerous-html` candidate.

### Fixed

- **`fallow security` no longer misses sinks on complex objects and named `vm` imports.** These shapes were silent before and are now candidates:
  - an `innerHTML` or `outerHTML` assignment on `this.node`, `this.shadowRoot!`, `el!`, `(el as T)`, an optional chain or a call result such as `document.getElementById(id)`;
  - `insertAdjacentHTML` and `createContextualFragment` on a call result;
  - `runInNewContext`, `runInThisContext` and `runInContext` from a named import of `vm` or `node:vm`, or from `require("vm")`. The callee is `node:vm.<member>`;
  - `new Function(...names, body)` and its local alias `const F = Function`, where a spread fills the parameter list.
