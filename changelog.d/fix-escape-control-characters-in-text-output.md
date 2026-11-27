### Fixed

- **Text output keeps each project string on its line.** A file path, an import specifier or an export name can hold a line break or another control character. The human, compact and Markdown formats, the `Gates:` note and the `parse-error` gate lines on stderr now print such a character as a visible escape, for example `\n` or `\u{001B}`. Before, a line break in a name started a new output line, and a CI runner could read that line as a workflow command. JSON output keeps the raw string.
