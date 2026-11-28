### Performance

- **Import resolution makes far fewer file system calls on large projects.**
  A resolution probe for a path that does not exist now costs one `lstat`, not
  a `realpath` lookup of each path component. The index of symlinked project
  files now resolves a directory once for all its files. On the next.js
  repository, `fallow dead-code` with all threads spends about 23% less system
  time and about 10% less CPU time, with the same output.
