### Fixed

- **`fallow fix` skips a file with a bare CR line ending.** The fixers split lines on LF, but the parser also ends a line at a bare CR. An edit in such a file used offsets from the wrong line. The file is now skipped with `skip_reason: "mixed_line_endings"`, as a file with mixed CRLF and LF line endings is, and the run exits with code 2.
