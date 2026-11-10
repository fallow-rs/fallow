### Fixed

- **`--type-aware` no longer confirms a setter as used because of a read, or a getter because of a write.** TypeScript gives a `get` and `set` accessor pair one symbol, so a read such as `if (mod.file)` also confirmed the setter, and the refined report dropped it. The semantic pass now credits a read to the getter, a plain assignment to the setter, and a compound assignment such as `+=` to both. A setter that the code only reads is now reported with and without `--type-aware`.
