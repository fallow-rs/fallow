### Fixed

- **Negative `import.meta.glob` entries now exclude files.** Vite reads an
  array entry such as `'!./pages/skip.js'` or `'!**/*.test.ts'` as an exclude
  for every positive entry of the same call. Before, Fallow ignored these
  entries. The excluded files then counted as loaded, and Fallow did not
  report them as unused. Now Fallow applies each negative entry that starts
  with `./`, `../` or `**`.
- **The literal text of a dynamic import pattern now matches only itself.**
  Before, Fallow read glob characters in the static parts of a template
  literal, a string concatenation, or a `require.context` directory as glob
  syntax. For example, ``import(`./[id]/${name}.js`)`` matched `./i/view.js`
  and not `./[id]/view.js`, and `import('./{group}/' + name)` matched no file.
  Now the characters `*`, `?`, `[`, `]`, `{`, `}` and `(` in these parts match
  only themselves. The extraction and graph cache versions change, so the
  first run after the upgrade parses the project again.
