### Fixed

- **`unused-class-members` no longer reports a used member as unused when a destructure of an imported instance or of a class field is its only read.** This applies with and without `--type-aware`. In each example, `Service.m` is now used:
  - A destructure of an instance from another module, such as `const { m } = service` after `import { service } from './service'`.
  - A destructure of a class field, such as `const { m } = this.service` in a class with `constructor(private service: Service)`.
  - The assignment, parameter default and `typeof` forms of these destructures, such as `({ m } = service)`, `run({ m } = this.service)` and `({ m }: typeof service) => m()`.
- **`unused-exports` no longer reports the exports of a namespace import as unused when a destructure with a computed key reads it.** A computed key, such as `const { [name]: value } = ns`, can read any export, so all exports of `ns` are now used.
