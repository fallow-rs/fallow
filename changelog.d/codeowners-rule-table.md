### Fixed

- **`--group-by section` merges repeated GitLab CODEOWNERS section headers.** Headers with the same name, compared without case, now form one group with the name of the first header. The `owners` of the group are the default owners of all its headers, in file order. Before, headers that differ only in case gave two groups, and the `owners` of a repeated header came from one header only. A `--group` filter or a saved health group baseline that names a later spelling of a header no longer matches.
- **A CODEOWNERS file that starts with a UTF-8 byte order mark keeps its first line.** Before, the first pattern or section header included the mark and matched no file.
