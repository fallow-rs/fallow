---
name: binary-loop
description: Iteratively reduce Fallow binary size using cargo-bloat and release-build measurements while preserving features, performance, and compatibility.
---
<!-- Generated from .agents/skills. Do not edit. -->

# Binary size loop

1. Measure the release binary and capture cargo-bloat evidence.
2. Select one dependency, monomorphization, feature, or codegen contributor.
3. Implement a bounded change.
4. Rebuild with identical release settings.
5. Keep the change only when size improves and verification passes.
6. Repeat until the target is met or remaining candidates have poor tradeoffs.
7. Run `review`.

Do not trade away supported targets or public behavior without explicit scope.
