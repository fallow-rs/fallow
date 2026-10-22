### Fixed

- **`unused-class-members` credits members that a nested destructure reads.** Without `--type-aware`, `({ k: { m } } = { k: service })`, `({ k: { m } } = holder)` and `({ k: { m } = service } = {})` did not credit `Service.m`, so Fallow reported a used member as unused. The `const` forms had the same gap. Fallow now follows a nested pattern into an object literal property, a default value and a static member path. The member path form reads `holder.k.m` in the same way as the member expression, so it credits a member only when Fallow knows the class of `holder.k`.
