# Shared-state inventory

`fallow architecture --shared-state` requests a syntactic inventory. With no
other selector it reports only the inventory. `--cycles`, `--boundaries` and
`--policy` add their ordinary findings. Runs without `--shared-state` retain
ordinary extraction and selection.

The optional `shared_state` body has its own version. It is outside finding
arrays, totals, severity promotion, health scoring, baseline counts, finding
gates and automatic fixes. Parse, completeness and baseline validity gates
still apply. Human, JSON and Markdown output support it. Saved envelopes can
render it as Markdown. Other formats, `--sarif-file`, `--finding-id` and
`--group-by` refuse the requested inventory, including an empty body.

Imported writes require a direct runtime ESM import and a unique value-export
declaration through the existing graph resolver. Assignment, compound/logical
assignment, update and delete preserve static property segments. A literal
`["a.b"]` is one segment; `.a.b` is two. Namespace `ns.config.x` resolves the
exported `config` value; assigning `ns.config` is excluded. Type-only imports,
ambiguous or unresolved origins, local aliases, dynamic keys, mutator calls
and `Object.assign` are outside the contract. Destructuring and iteration
assignment targets are excluded; their computed keys, default initializers and
right-hand expressions can still contribute reads. The declaration is the
binding origin, not proof of ownership of the runtime object.

Globals group the exact unshadowed root spelling (`window`, `globalThis`,
`self`, `global`) and direct custom slot. Each group retains separate read and
write lists with sites in different non-test files. Ambient declarations and
import connectivity do not suppress evidence. Functions need not have been
called. `same_runtime` and `execution_order` remain `unknown`.

`fallow-ignore-next-line shared-state` and `fallow-ignore-file shared-state`
are recognized even on ordinary runs. Suppressions and `ignoreFindings` apply
before grouping. `sharedStateAllowlist` contains exact root/slot pairs, for
example `[["window", "analyticsQueue"]]`. Test and test-support files are
excluded. File, workspace and diff scope retain an entire global group when
one of its sites matches, preserving the evidence for the observation.

The explicit builtin catalogue is in
[`shared_state_builtins.rs`](../crates/engine/src/shared_state_builtins.rs).
It distinguishes ECMAScript globals, browser slots and Node globals. Its
references are [ECMAScript global properties](https://tc39.es/ecma262/#sec-global-object),
[Window](https://developer.mozilla.org/en-US/docs/Web/API/Window),
[WorkerGlobalScope](https://developer.mozilla.org/en-US/docs/Web/API/WorkerGlobalScope)
and [Node globals](https://nodejs.org/api/globals.html). `globalThis` excludes
the host union without inferring which runtime will execute a file. The
catalogue is explicit and incomplete; declarations never extend it.

Extraction retains at most 4096 facts per file and walks at most 64 static
property segments. The engine retains at most 20000 source facts. Fact and
property-depth limits set `truncated`. Parser diagnostics, nesting limits,
recorded discovery/read loss and truncation clear `complete`. Vue/Svelte scripts and Astro frontmatter
and processed scripts preserve their parser diagnostics through disk and
process reuse. Ordinary runs also report these embedded parse errors.
Unsupported syntax, unprocessed Astro scripts and MDX prose remain outside
the extraction contract; their intentional exclusion does not imply data loss. There are no
writer-by-reader pairs and no source previews. IDs are deterministic from
root-relative source sites with forward slashes, binding identity, operation
and exact property segments; moving a site can change its ID.

The extraction mode is part of immutable resolved configuration and of the
cache configuration identity. This separates normal and inventory parses in
disk, warm-process and disk-disabled session reuse. Embedded facts remap byte
offsets and import indices during merging. The graph resolver and graph edges
are unchanged. Graph restoration already reattaches parsed semantic facts, so
adding these facts does not change its persisted graph contract.
