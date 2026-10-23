### Performance

- **Long heritage names and long dotted component tags in Astro and MDX files
  are now linear to analyze.** Each level of `interface X extends a.b.c...` or
  `<a.b.c... />` recorded the full name of its receiver, so the time and the
  output grew with the square of the name length. For example, an interface
  heritage name of 20000 links took 110 seconds and now takes 0.08 seconds. A
  receiver longer than 64 links is not recorded, the same limit that member
  access names use. The names of `class C extends a.b.c...` and
  `implements A.B.C...` are now built once, instead of a copy of the name at
  each link. The cache version changes, so the first run after the upgrade
  parses all files again.
