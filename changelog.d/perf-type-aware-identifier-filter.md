### Performance

- **`--type-aware` asks the checker only about identifiers that can reach a query.** The symbol-use scan sent every identifier of every TypeScript file to the checker. It now sends only the names of the queried symbols and the names that imports and exports of the project declare. JavaScript files keep the full scan, because `require` bindings also create aliases. The decisions stay the same.
