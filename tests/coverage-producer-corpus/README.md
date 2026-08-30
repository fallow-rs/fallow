# Coverage producer conformance

`crates/engine/src/health/scoring.rs` maps every function Fallow extracts onto
a record in an Istanbul coverage map. The whole matcher rests on one thing:
WHERE each producer anchors a function record. Every other geometry expectation
in the repository is a hand-written JSON literal inside a Rust test, so nothing
notices when a producer moves an anchor in a minor release.

This corpus records what real producers actually emit, and asserts what Fallow
does with it. The recorded maps are also the reference the next hand-written
fixture can be checked against, by a reader: nothing here polices Rust
literals.

## What is asserted, and what is not

| Layer | Artifact | Asserted |
| --- | --- | --- |
| Behavioral census | `manifest.json` `census[]` | Yes. This is the gate. |
| Producer geometry | `maps/<producer>/<probe>.json` | No. It is the review artifact, pinned by sha256. |
| Self-test | computed each run | Yes for line drift and for columns moved past the end of their line. A collapse to the line start is recorded, not asserted: the matcher tolerates bounded column drift by design. |

The census is the sorted per-unit list of
`{name, line, col, coverage_source, coverage_pct}` that `fallow health` reports
for one probe against one recorded map. Each probe function is written to have
a distinct executed-over-total statement ratio, so `coverage_pct` usually acts
as a per-record fingerprint: a matching census then proves WHICH record
resolved, not merely that some record matched. Where a producer cannot give two
records distinct ratios, the manifest records the collision under
`fingerprint_collisions` and the self-test escalates that row to moving every
record at once, because moving one would be invisible.

Geometry is deliberately not asserted. A producer that moves an anchor the
matcher absorbs shows up as a map diff with an unchanged census, which reviews
at a glance. Pinning coordinates instead would turn every harmless producer
bump into a refresh chore, and refresh chores get rubber-stamped.

Nothing in this corpus asserts `summary.coverage_model`, the health score, the
presence of a CRAP score, or a matched/total scalar. Those signals stay
constant across the geometry moves this corpus exists to detect, so a check
built on them cannot fail.

## The matrix

Five rows over four packages. Rows are data in `manifest.json`, not code:
adding one is a manifest entry plus a driver module under
`producers/drivers/`.

| Row | Package | Profile | Real consumers |
| --- | --- | --- | --- |
| `istanbul-lib-instrument` | istanbul-lib-instrument | default | Jest, Vitest with the istanbul provider |
| `v8-to-istanbul` | v8-to-istanbul | default | c8, nyc in V8 mode, older Vitest |
| `ast-v8-to-istanbul` | ast-v8-to-istanbul | default | Vitest's current default provider |
| `oxc-coverage-instrument` | oxc-coverage-instrument | default | this project's own instrumenter |
| `oxc-coverage-instrument-istanbul` | oxc-coverage-instrument | `compat: "istanbul"` | this project's own instrumenter |

The two `oxc-coverage-instrument` rows are flagged `self_conformance: true`.
They are this project's own package, so their agreement with each other is
weaker independent evidence than agreement with a V8-derived row.

## The probes

Chosen by measured divergence, not by taxonomy. Each probe carries the
`invariants` it defends, and a census delta names the Rust function that just
lost its evidence.

- `plain` baseline function declarations.
- `named-fn-expr` istanbul-lib-instrument anchors `decl` at the identifier
  (column 23), v8-to-istanbul at the `function` keyword (column 14): a
  nine-column split on one construct. `oxc-coverage-instrument`'s default
  profile names the record after the binding, and both units go unattributed.
- `curried-arrow` record N's `loc.start` lands exactly on record N+1's
  `decl.start`. v8-to-istanbul emits a single record for the whole chain, so
  two of the three arrows are correctly unattributed.
- `accessors` the same getter is spelled three ways across the matrix:
  `(anonymous_N)`, `get area` and `area`. The setter beside it lands on
  `area_2` under ast-v8-to-istanbul, which is its own divergence: the two
  accessors of one property become two records whose names differ only by a
  suffix the matcher never sees on the extraction side.
- `one-line` `ast-v8-to-istanbul` reports an end column it cannot place as
  `Infinity`, which serializes to JSON `null` and deserializes to `0`, so a
  one-line body span inverts. No hand-written fixture would express this.
- `bare-if` v8-to-istanbul records `column: -1` for the implicit else it cannot
  place. Positions are unsigned, so a strict parse rejects the whole map and
  every unit in the file silently falls back to its static estimate.
- `decorated` istanbul-lib-instrument records `decl.start` at the decorator and
  `loc.start` at the body brace, so the position Fallow extracts sits strictly
  between them. `oxc-coverage-instrument` anchors `decl` at the method name
  instead, one line lower.

`decorated.ts` cannot be executed by Node, so the two V8-derived rows record no
map for it. That is a producer capability fact, recorded as the absence of a
map rather than as a miss.

## Recorded misses are expected values

A unit that resolves to no record is an `estimated` census entry carrying a
hand-written `rationale` naming the producer version and the behavior. The gate
fires on a census DELTA, never on the existence of a miss. Six such entries
exist today.

## Provenance

`manifest.json` records the sha256 of every probe, every recorded map, and the
producer lockfile, plus the Node the corpus was recorded on. The lockfile
digest is the real pin: `istanbul-lib-instrument` 6.0.3 declares
`"@babel/parser": "^7.23.9"`, so the same nominal version emits different
geometry on different days. `node_pin` is provenance rather than a gate: two
rows derive their record set from V8 `ScriptCoverage`, which is a property of
the Node version, but the recorded maps are byte-identical on Node 22.21.1,
22.23.2, 24.18.0 and 26.7.0. Both re-recording commands say so when they run on
a different Node, and neither refuses.

The census runs under `.fallowrc.jsonc` in this directory, passed as
`--config`. Without it Fallow would walk up from the probe and load the
repository's own config, and a census that answers to that file is not a
statement about the matcher.

## Running it

The gate needs only the binary. It reads committed maps and installs nothing:

```bash
cargo build -p fallow-cli --bin fallow
npm run check:coverage-producers
```

Re-recording needs the pinned producers:

```bash
npm ci --prefix tests/coverage-producer-corpus/producers \
  --no-audit --no-fund --ignore-scripts
npm run check:coverage-producer-drift   # compare, never write
npm run refresh:coverage-producers      # re-record
```

Geometry moved and the census is unchanged: land it, the map diff IS the
change. The census regressed: that is a matcher bug, fix
`crates/engine/src/health/scoring.rs` and refresh again. There is deliberately
no flag that accepts a census regression, no flag that rewrites the census to
match observed behavior, and no flag that turns a failure into a report: every
command here exits non-zero on every finding it makes.

A row whose producer stops emitting a map is the one drift re-recording refuses
to absorb. Dropping it would retire the row, and the invariants it carries,
behind a green build, so both commands stop and name the row. Retiring a row is
a manifest edit a human makes on purpose.

## What this does not cover

The corpus covers constructs someone thought to write. JSX, Vue single-file
components, class static blocks, standards decorators and source-mapped
TypeScript builds are absent, and Bun, Deno and swc-based instrumenters are
outside the matrix. Re-recording is a maintainer command rather than a CI job,
and it is never run on Windows, which is where CRLF column drift is most
likely. The `.gitattributes` in this directory is what keeps a CRLF checkout
from moving every recorded column.

The census is column-aware, not column-exact. The matcher normalizes small
column drift on purpose, so a producer that re-anchors `decl` from the
identifier to the `function` keyword keeps its census and shows up as a map
diff instead. The self-test proves each row reads the column at all by moving
every recorded column past the end of every line in its probe and requiring the
census to fail; it does not prove a one-column move would be caught, because
the matcher is built to absorb one.
