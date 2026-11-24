### Fixed

- **An exported `Props` type in a `.astro` file is no longer an unused type.** Astro reads the `Props` export of a component to type `Astro.props`, so no other file imports it. A `Props` export in a `.ts` file still gets the normal check.
