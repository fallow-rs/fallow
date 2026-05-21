# ADR-009: Cache config-hash ingredient set

**Status:** Accepted
**Date:** 2026-05-21

## Context

`crates/extract/src/cache/` is an incremental on-disk cache keyed by `(file_path, content_hash)`. Before #466, the only escape hatch for invalidating stale entries across a fallow upgrade was a `CACHE_VERSION` bump. A user-driven config change (disabling a plugin, removing an inline `framework: [...]` entry) did NOT invalidate the cache; the per-file `ExtractedFile` bytes were unchanged, but downstream consumers could be silently short-circuited by stale extraction artifacts. The fix lands a config-hash stored in the cache header: on load, a mismatch returns `None` and the next run re-extracts.

The question this ADR answers: **which config fields participate in the hash, and which do not?**

## Decision

**Today's ingredients (narrow set):**

1. Active external plugin names. `discover_external_plugins` finalizes the plugin set after merging inline `framework: [...]` definitions; the hash sees the post-merge list.

The set is computed by `compute_cache_config_hash(&[ExternalPluginDef])` in `crates/config/src/config/resolution.rs`. Names are sorted before hashing for deterministic output across runs (FxHashMap iteration order is not stable across processes).

**Hash algorithm:** xxh3-64 over length-prefixed bytes (`u32 LE` length followed by UTF-8 name bytes). Length-prefixing keeps `["ab", "c"]` and `["a", "bc"]` distinct even though the concatenated bytes are identical.

**`no_cache: true` short-circuits to 0.** When caching is disabled, the bookkeeping is skipped entirely.

## Alternatives considered

### Wide ingredient set (include `entry`, `ignorePatterns`)

The issue body that motivated #466 explicitly cited `entry` / `ignorePatterns` changes as motivation. We chose NOT to include them because:

- **They are detection inputs, not extraction inputs.** Extraction output is shape-identical regardless of which files are in-scope. Detection-side fields belong on the analysis layer, not in the cache key.
- **Users edit `ignorePatterns` casually during refactors.** A wide hash creates daily false cache invalidations on routine edits.
- **The narrow set is a load-bearing contract.** Future contributors who add an extraction-affecting config field MUST extend the hash function or their change ships with silent staleness; documenting the narrow set in code + ADR makes the contract visible.

### Per-file partial invalidation (hash on each `CachedModule`)

Each entry could carry its own config-hash, allowing partial invalidation when only some plugins flip. Rejected because:

- Increases on-disk size per entry (8 bytes * 100k entries = 800 KB; not catastrophic but unnecessary).
- Whole-cache invalidation already matches `CACHE_VERSION` semantics; users understand and expect "config changed -> cache rebuilds from scratch."
- Re-extraction is fast (incremental parse, content-hash short-circuit). The cost of a full rebuild after a config change is bounded by I/O, not analyzer work.

### Hashing plugin file *content* (not just names)

A contributor who edits `.fallow/plugins/foo.json` without renaming silently keeps the cache. Plugin content hashing would catch this. Rejected for v1 because:

- Reading every discovered plugin file at config-resolve time has a non-trivial cost (every CLI invocation, regardless of cache state).
- Plugin file edits are rare in practice (users either install plugins via dependencies or write inline `framework: [...]` which IS hashed).
- Documented as a known limitation; users can work around with `--no-cache` for plugin-file edits.

If reports surface, revisit by adding the plugin file content hash to the ingredient set behind a feature flag.

## Adding a new ingredient

Future contributors who add a `ResolvedConfig` field that affects extraction output bytes MUST:

1. Update `compute_cache_config_hash` in `crates/config/src/config/resolution.rs` to fold the new field in. Maintain the "sort before hash" pattern for determinism.
2. Document the field's role in this ADR's "Today's ingredients" list.
3. Decide whether the change also warrants a `CACHE_VERSION` bump:
   - `CACHE_VERSION` bumps on **format changes** (new field on `CachedModule`, schema migration).
   - Config-hash ingredient updates do NOT need a version bump because old caches with mismatched hashes are already discarded as part of normal `load` operation.

The signal for "does this field affect extraction" is "field affects extraction output bytes," not "field affects detection behavior." Detection-only fields (entry, ignorePatterns, severity overrides, rule toggles, exclusions) belong on the analysis layer.

## Consequences

**Positive:**

- Config changes invalidate the cache automatically; no more silent staleness across plugin / framework edits.
- The narrow set keeps false invalidations rare.
- The contract is documented and testable.

**Negative:**

- Plugin file content edits don't invalidate the cache (documented limitation; mitigate with `--no-cache`).
- A contributor who adds an extraction-affecting field and forgets the hash function has a silent failure mode. Mitigated by code comments + this ADR + future code review prompts.
