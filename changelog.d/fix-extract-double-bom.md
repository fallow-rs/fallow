### Fixed

- **A file that starts with two U+FEFF characters gets correct spans.** Only the first U+FEFF of a file is the UTF-8 byte order mark. The second U+FEFF is source text. Extraction removed both, so each span of the file was 3 bytes too early. For `\u{feff}\u{feff}export const a0 = 1;`, the span of `a0` held `st`. Extraction now removes the mark one time only. The extraction cache version and the graph cache version change, so a warm cache does not give the old spans.
