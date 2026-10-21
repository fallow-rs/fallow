### Fixed

- **`fallow dead-code --type-aware` no longer confirms a used class member or
  export as unused when a shorthand name reads it.** The type-aware check now
  finds a class member that a shorthand destructure reads, for example
  `const { run } = service`, `({ run }: Service) => run()` or
  `({ run } = service)`. It also finds an export that a shorthand object
  property reads, for example `{ value }`, and an export that a destructure of
  a namespace import reads, for example `const { value } = ns`. Before this
  fix, a used member that only a parameter destructure or an assignment
  destructure read could get a guarded `remove-class-member` fix. For an array or loop
  destructure, the check now abstains with `dynamic-member-access`.
