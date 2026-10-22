### Fixed

- **`unused-class-members` credits more destructure reads.** Without `--type-aware`, Fallow reported a used member as unused when one of these forms was its only read. Fallow now credits `Service.m` for each form:
  - A `for...of` destructure, such as `for (const { k: { m } } of [{ k: service }])`, `for ({ m } of [service])`, `for (const { m } of services)` with `services: Service[]` or `for (const { m } of [...services])`.
  - A parameter destructure with a default value, such as `function run({ m } = service)` or `function run({ k: { m } } = { k: service })`.
  - A parameter annotated with `typeof`, such as `({ k: { m } }: typeof holder)` or `({ k: { m } }: { k: typeof service })`.
  - A getter or a spread in the source object, such as `{ get k() { return service } }` or `{ ...holder }`.
