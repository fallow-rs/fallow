# ADR-010: `fallow-v8-coverage` and `oxc_coverage_v8` stay separate

**Status:** Accepted
**Date:** 2026-05-29

## Context

fallow ships `fallow-v8-coverage`, which converts a V8 `ScriptCoverage` dump (as
emitted by `node --experimental-test-coverage`, `c8`, or the Inspector protocol)
into line/column positions for runtime-coverage scoring. Separately,
`fallow-rs/oxc-coverage-instrument` ships `oxc_coverage_v8`, a Rust-native
`v8-to-istanbul` equivalent that fills a full Istanbul `FileCoverage`
(statements, functions, branches) from V8 ranges.

The concern (issue #509) was accidental duplicated correctness logic: two V8
offset mappers that could drift on offset semantics, wrapper offsets, and
source-map handling. The question was whether to consolidate them.

## Decision

Keep both crates. The split is intentional, not accidental duplication.

They solve inverse problems in opposite unit spaces:

- `fallow-v8-coverage` is a **forward constructor**. Given a V8 dump plus the
  source text, it maps V8 source offsets to positions. It does not parse the
  source. V8 reports offsets in **UTF-16 code units** (V8 strings are UTF-16),
  so `LineOffsetTable` stores line starts in UTF-16 units.
- `oxc_coverage_v8` is a **count filler**. It requires a caller-built Istanbul
  `FileCoverage` (produced by an AST instrumentation pass) and fills its
  `s`/`f`/`b` vectors by converting Istanbul positions to **byte** offsets and
  intersecting V8 ranges.

Two follow-on cleanups landed with this decision:

1. Deleted the dead Istanbul emitter from `fallow-v8-coverage` (`normalize_script`,
   `ScriptInput`, `IstanbulFileCoverage`, `IstanbulFunction`, `IstanbulRange`).
   It was never consumed by fallow: the CLI builds its own remapped output in
   `crates/cli/src/health/coverage.rs` from the input structs plus
   `LineOffsetTable`, and CRAP scoring uses a separate local `IstanbulFileCoverage`
   in `crates/cli/src/health/scoring.rs`. The crate is now exactly "V8 dump
   parser + UTF-16 offset mapper". This removed the only apparent overlap.
2. Hardened the UTF-16 conformance test to assert a within-line column (where the
   UTF-16-vs-byte distinction is observable) rather than a line start, so the
   boundary cannot silently drift.

## Empirical basis

Measured against real Node v22.22.1 via `Profiler.takePreciseCoverage`:

- **UTF-16 offsets.** For `const e = "😀😀"; function f(){...}` on one line, V8
  reports `f.startOffset = 18` (the UTF-16 index of `function`), not the byte
  index 22, and the module range `endOffset = 49` (UTF-16 length), not 53 bytes.
  fallow's UTF-16 mapper is correct; a byte model mismaps any construct preceded
  by a non-ASCII character on the same line.
- **CJS wrapper.** A `require()`'d module reports `topLevel.startOffset = 0`
  (raw-source index 0); modern Node already strips the module wrapper from the
  offset space, so fallow needs no `wrapper_length`.

## Alternatives considered

### Option A: Replace `fallow-v8-coverage` with `oxc_coverage_v8`

Rejected. It is both a correctness regression and a structural cost increase.
`oxc_coverage_v8`'s byte-offset assumption is wrong for real Node dumps (UTF-16),
and it requires a caller-built `FileCoverage`, forcing fallow to AST-parse and
instrument every covered file just to fill statement/branch counts it discards
(CRAP only consumes `fnMap` + `f`). That pulls `oxc_coverage_instrument` +
`oxc_parser` into the runtime-coverage path for data fallow throws away.

### Option B: Extract a shared low-level offset helper

Rejected. The two mappers run in opposite directions and opposite unit spaces;
there is no clean shared primitive without forcing one to adopt the other's unit
convention. A cross-repo dependency edge (fallow to oxc-coverage-instrument)
coupling release cadences is not justified by ~40 lines of well-tested,
property-tested code.

## Consequences

- The boundary is explicit: fallow owns Node V8 dump ingestion in UTF-16 space;
  `oxc_coverage_v8` owns Istanbul count-filling for AST-instrumented coverage in
  byte space. The module docs of `fallow-v8-coverage` reference this ADR.
- `fallow-v8-coverage`'s published public surface shrank by the dead emitter
  types. The crate is published (lockstep workspace version) but is consumed only
  by `fallow-cli`; the removal is a deliberate cleanup with no known external
  consumers, noted in the changelog.
- The protocol/output shape the closed-source `fallow-cov` sidecar consumes is
  unchanged; this decision touched no wire format.

### Known limitations (not addressed here)

These are producer/source edge cases the offset mapper does not currently
compensate for, listed so the next person touching `line_offsets_for_script`
knows the gap exists:

- A leading shebang (`#!/usr/bin/env node`) stripped by Node before V8 sees the
  source shifts all offsets relative to the on-disk file fallow reads.
- A UTF-8 BOM is one byte vs. one UTF-16 unit at the head of the file.
- CRLF combined with a multibyte character on the same line is exercised by the
  proptest alphabet but not by a named fixture.
