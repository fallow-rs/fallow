---
name: release
description: Prepare and publish a Fallow release with version, changelog, generated contracts, companion repositories, registry publication, and post-release verification.
---
<!-- Generated from .agents/skills. Do not edit. -->

# Release

1. Read `docs/development/quality-gates.md` and the release section of
   `docs/development/task-context-map.md`.
2. Confirm `main` is clean, current, reviewed, and green.
3. Verify the unreleased changelog against commits since the prior tag.
4. Regenerate public contracts and synchronize `fallow-docs` and
   `fallow-skills`.
5. Apply version changes transactionally and run package dry-runs.
6. Create signed release commits and tags.
7. Monitor all publication, registry, companion, and deployment workflows.
8. Verify released binaries, packages, docs, schemas, and public health from
   their real endpoints.

Do not report a release complete while any required publication or verification
workflow is pending.
