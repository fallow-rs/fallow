### Performance

- **A bare `fallow` run with health keeps one copy of the parsed modules.** The health analysis of a combined run read a copy of all parsed modules, and the analysis session kept the original until it was freed. Health now reads the modules of the session, which are the same modules that `fallow health` reads. On next.js the peak memory footprint went down by 4.3%, and on preact by 3.3%. The JSON output of next.js, babel, vue-core, preact and zod did not change.
