### Fixed

- **`fallow fix` skips a file with a bare CR line ending.** The fixers split lines on LF, but the parser also ends a line at a bare CR. An edit in such a file used offsets from the wrong line. The file is now skipped with `skip_reason: "mixed_line_endings"`, as a file with mixed CRLF and LF line endings is, and the run exits with code 2.
- **`fallow fix` keeps the end of a class field when it removes the next method.** A field with no `;`, such as `value = 1`, ends at the line break only because the next member cannot continue it. When the removed method was that member, a following member that starts with `[`, `*` or `(` joined the field, so the class did not parse or changed meaning. The fixer now puts a `;` in front of that member.
