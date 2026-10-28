### Fixed

- **A Vite alias with a regular expression `find` now resolves.** Before, fallow read `find` only as a string. An alias such as `{ find: /^node:module$/, replacement: p('./src/shim.ts') }` was dropped, so the import of `node:module` did not reach the replacement file and the file was reported as unused. Fallow now reads two regex shapes:
  - An anchored literal, such as `/^node:module$/` or `/^@scope\/pkg$/`. The alias key is the literal.
  - An anchored prefix with one capture of the rest, such as `/^@\/(.*)$/`, when the replacement ends with `/$1`. The alias key is the prefix, and the replacement is the directory before `/$1`.

  A replacement can also be a template literal that starts with a path call, such as `` `${path.resolve(__dirname, 'src')}/$1` ``. Other regex shapes, a regex with a flag other than `u` or `v`, and a replacement that uses a capture in another position still give no alias, because fallow matches aliases by prefix. Thanks [@VariableVince](https://github.com/VariableVince) for the report.
