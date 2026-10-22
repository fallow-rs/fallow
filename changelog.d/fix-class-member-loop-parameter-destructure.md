### Fixed

- **`unused-class-members` no longer reports a used member as unused when one of these destructure forms is its only read.** This applies with and without `--type-aware`. In each example, `Service.m` is now used:
  - A `for...of` destructure, such as `for (const { k: { m } } of [{ k: service }])`, `for ({ m } of [service])`, `for (const { m } of services)` with `services: Service[]` or `for (const { m } of [...services])`.
  - A parameter destructure with a default value, such as `function run({ m } = service)` or `function run({ k: { m } } = { k: service })`.
  - A parameter annotated with `typeof`, such as `({ k: { m } }: typeof holder)` or `({ k: { m } }: { k: typeof service })`.
  - A getter or a spread in the source object, such as `{ get k() { return service } }` or `{ ...holder }`.
