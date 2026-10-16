### Performance

- **Every run compiles its globs and regular expressions faster.** The
  logging filter pulled in a regular-expression feature that built a full
  automaton for each small pattern. Each glob compile cost about three times
  as much, and Fallow compiles hundreds of them in a run. A dead-code run now
  takes about 5 to 7 percent fewer instructions on a mid-size project, and
  the binary is smaller. `RUST_LOG` still reads `target=level` and `level`
  directives. It skips span and field directives, which it read before.
