### Fixed

- **`fallow security` no longer misses `vm.Script`, `vm.compileFunction` and `<script>` `setAttribute("src", x)`.** These shapes were silent before and are now `code-injection` candidates when the code argument is not a literal:
  - `new vm.Script(code)`, and `new Script(code)` after `import { Script } from "vm"` or `require("vm")`. The callee is `node:vm.Script` for a named or `require` binding;
  - `vm.compileFunction(code, params)`, with the same `node:vm.compileFunction` callee for a named or `require` binding;
  - `el.setAttribute("src", url)` on a local bound to `document.createElement("script")`, with the callee `HTMLScriptElement.src`. The same call on another element, such as `img.setAttribute("src", url)`, stays quiet.
