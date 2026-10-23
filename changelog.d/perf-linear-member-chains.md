### Performance

- **Long member, `await`, `+`, JSX tag and fluent call chains are now linear
  to analyze.** A file with a chain of many thousand links took minutes, or
  used gigabytes of memory, because several path builders copied the whole
  path at each link or scanned a growing list at each step. For example,
  160000 nested `await` took 33 seconds and now take 0.04 seconds, and a
  fluent chain of 4000 calls with different method names used 700 MB. A
  fluent chain now gives one fact for the chain instead of one fact for each
  call. A taint source path or a JSX tag receiver longer than 64 links is not
  recorded, the same limit that member access names use. The cache version
  changes, so the first run after the upgrade parses all files again.
