### Fixed

- **`fallow health --css` credits classes that plain `.js` and `.ts` DOM code applies.** Before, a class was reported as `unreferenced-css-class` when only a `.js`, `.ts`, `.mjs` or `.cjs` file applied it. Fallow now reads the values of `el.className = ...`, `el.classList.add / remove / toggle / replace / contains(...)`, `el.setAttribute('class', ...)` and `class:` or `className:` object properties. A template value such as `` `panel-${kind}` `` credits its `panel-` prefix. Other strings in these files still do not credit a class. A `.js` file that holds JSX is now scanned the same as a `.jsx` file. Thanks to [@osazemeu](https://github.com/osazemeu) for the report and the clear reproduction.
  (Closes [#3865](https://github.com/fallow-rs/fallow/issues/3865))
