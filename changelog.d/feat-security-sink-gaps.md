### Added

- **`fallow security` reports iframe `srcdoc` as a `dangerous-html` sink.** An iframe renders its `srcdoc` value as a full HTML document, so a non-literal value is the same XSS class as `innerHTML`. Fallow now reports `frame.srcdoc = html`, `frame.setAttribute("srcdoc", html)`, JSX `srcDoc={html}`, Vue `:srcdoc`, Svelte `srcdoc={html}` and Angular `[srcdoc]`. A static value stays quiet.

### Fixed

- **`fallow security` no longer misses code and HTML sinks behind evasion syntax.** These shapes were silent before and are now candidates: `(0, eval)(code)`, `globalThis.eval(code)`, `window.eval(code)`, `self.eval(code)`, `window["ev" + "al"](code)`, `const e = eval; e(code)`, `Function(code)`, `Function.prototype.constructor(code)`, `value.constructor.constructor(code)`, `setTimeout("run(" + id + ")")`, `el["innerHTML"] = html`, `el["inner" + "HTML"] = html`, and a JSX spread that carries `dangerouslySetInnerHTML` inline or through a local object literal. An assignment whose property name joins a string with a dynamic part (`el[prefix + "HTML"] = html`) cannot be resolved, so `fallow security blind-spots` now lists it.
