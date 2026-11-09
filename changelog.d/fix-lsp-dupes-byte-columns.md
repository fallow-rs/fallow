### Fixed

- **Clone locations are correct on a line with non-ASCII text.** The duplication detector gave `start_col` and `end_col` as character counts, but the LSP read them as byte columns. Thus a non-ASCII character before a clone on the same line moved the clone diagnostic and hover in the editor. The detector now gives byte columns, the same as the other analyses. A consumer that read these JSON fields as character columns must change. (Closes [#4378](https://github.com/fallow-rs/fallow/issues/4378))
