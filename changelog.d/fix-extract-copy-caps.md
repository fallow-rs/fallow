### Performance

- **Extraction copies a bounded amount of data for long chains, long callees and repeated names.** Before, some source shapes made one file cost the product of two counts, or a power of two, in memory and time. Each shape now has a limit far above real code:
  - A module binding or a folded template literal keeps a static string of 16 KiB at most. A chain of template literals that each read the previous binding twice no longer doubles the value at each declaration.
  - Extraction records a call whose callee path is longer than 1 KiB as a callee that it cannot read. It does not copy the path once for each argument.
  - The members and heritage of a local class go to 32 export names of the class at most.
  - Glimmer template chains and CSS-in-JS token reads stop at 64 links, as JavaScript member chains already do.
  - The complexity name of a callback keeps a callee of 128 bytes at most. A longer dotted callee gives its last property.
  - A Playwright fixture type collects 512 fixture bindings at most, with paths of 128 bytes at most.
  - A Vue, Svelte or Angular template expression declares only the template locals that it names, not every local in scope. The template scans read the locals in scope without a copy at each element.

  The output for real projects does not change. The parse cache version changes, so the first run after the upgrade parses each file again.
