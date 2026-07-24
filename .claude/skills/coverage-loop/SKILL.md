---
name: coverage-loop
description: Iteratively improve Fallow Rust test coverage with cargo-llvm-cov, prioritizing meaningful untested behavior and preserving runtime correctness.
---
<!-- Generated from .agents/skills. Do not edit. -->

# Coverage loop

1. Capture a coverage baseline with the repository-supported command.
2. Select uncovered behavior by risk, not by easiest lines.
3. Add behavior-focused tests that survive refactors.
4. Re-run targeted tests and the same coverage command.
5. Keep only tests that improve meaningful coverage or lock a real invariant.
6. Repeat until the selected risk area is covered or remaining gaps require a
   separate design change.
7. Run `review`.

Do not add assertions that merely execute code without checking behavior.
