### Fixed

- **Dynamic import patterns that start with `../` now find their files.**
  Before this fix, `import.meta.glob('../components/*.js')`,
  `require.context('../widgets')`, a template literal such as
  ``import(`../locales/${locale}.js`)`` and a `'../views/' + name`
  concatenation matched no file. Fallow then reported each target as an
  unused file. Fallow now resolves each leading `../` step from the
  directory of the importing file, as Vite and webpack do. A negative
  `import.meta.glob` entry such as `'!../components/skip.js'` now also
  removes its file. A pattern that goes above the project root still matches
  no file.
