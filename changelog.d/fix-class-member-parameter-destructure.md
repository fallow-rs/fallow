### Fixed

- **`unused-class-members` credits members that a parameter destructure or a destructuring assignment reads.** Without `--type-aware`, `({ m }: Service) => m()` and `({ m } = service)` did not credit `Service.m`, so Fallow reported a used member as unused. Fallow now credits these reads in the same way as `const { m } = service`. The parameter form works for arrow functions, function declarations and methods, with renamed keys, defaults and nested type literals such as `({ svc: { m } }: { svc: Service })`. A rest element still marks the whole class as used.
