### Performance

- **The parse cache and the graph cache are written without `fsync`.** A run after a file change saves both caches, and on macOS the flush of the drive cache was most of the save. After a one-file edit, the parse cache save takes 5 to 9 ms less on small and medium projects. The write still goes through a temp file and a rename, so a reader sees the old file or the new file. A cache file that a crash cut short is refused at load, and the next run builds it again.
