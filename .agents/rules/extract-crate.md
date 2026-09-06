---
paths:
  - "crates/extract/**"
---

# Extraction

Follow the [extract guide](../../crates/extract/AGENTS.md) and the
[extraction reference](../../docs/reference/extract-internals.md) for current
parser, visitor, embedded-language and cache ownership.

Preserve original source offsets and syntactic provenance. Changes to cached
facts or extraction semantics require the cache-version update and validation
specified by those references.
