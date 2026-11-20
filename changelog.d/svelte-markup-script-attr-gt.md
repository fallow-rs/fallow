### Fixed

- **A `>` in a brace attribute of a Svelte markup `<script>` element no longer breaks the parse.** Svelte markup can hold a `<script>` element with an attribute expression such as `onload={() => {}}` or `data-big={2 > 1}`. Fallow ended the opening tag at the first `>`, read the rest of the expression as a script body, and reported the file as `source-parse-degraded`. That diagnostic then gave every finding of the run the `incomplete-import-graph` caveat. The script tag scan now skips brace expressions, also when they nest or hold quoted strings.
  (Closes [#4828](https://github.com/fallow-rs/fallow/issues/4828))
