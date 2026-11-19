### Fixed

- **A project package with only a `types` entry now resolves.** A package whose `package.json` has `types` or `typings` and no `main` or `exports` resolves through that entry. A type import of such a package was an unresolved import, and its dependency was unused.
- **`effect.schemaExportsAsTypes` reads `HttpApiSchema` transforms in `pipe`.** A schema such as `Schema.Struct({...}).pipe(HttpApiSchema.asFormUrlEncoded())` is now a schema. It stayed an unused export.
- **An Astro project no longer gets an unused dependency finding for `sharp`.** Astro loads it by name as the default image service of `astro:assets`. A config that uses `passthroughImageService` does not load `sharp`, so fallow still reports it there.
