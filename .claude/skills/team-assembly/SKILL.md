---
name: team-assembly
description: Select the minimum complete reviewer team for a Fallow change based on touched paths, contracts, integrations, and risk.
---
<!-- Generated from .agents/skills. Do not edit. -->

# Assemble reviewers

1. Read the current diff and `docs/development/review-routing.md`.
2. Map touched paths and behavioral effects to reviewer domains.
3. Include cross-cutting reviewers when contracts or multiple consumers change.
4. Brief reviewers with the diff, controlling plan, and primary source files.
5. Run independent reviews in parallel where their files do not overlap.
6. Synthesize verdicts without replacing blocks with majority votes.

Do not select reviewers only from filenames when behavior affects additional
consumers.
