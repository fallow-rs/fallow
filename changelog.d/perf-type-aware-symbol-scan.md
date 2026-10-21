### Performance

- **`--type-aware` class-member refinement is faster on large projects.** Since 3.18.0, the symbol-use scan sent extra checker requests and resolved file paths for every identifier in every file, also when the identifier could not match a query. The scan now checks the name first and keeps one alias answer for each symbol. On the TypeScript repository, the scan takes 4 to 12 s instead of 35 to 70 s on the same machine, with the same decisions.
