---
paths:
  - "crates/extract/**"
---

# Extraction constraints

- Read `docs/reference/extract-internals.md` for the affected syntax or parser.
- Keep cache keys and `CACHE_VERSION` aligned with serialized extraction data.
- Preserve byte offsets, source maps, BOM behavior, and framework embedding.
- Add fixtures for each syntactic form and a real-consumer smoke.
- Check reachability after extraction, not only visitor output.
