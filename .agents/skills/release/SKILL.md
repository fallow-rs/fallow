---
name: release
description: Prepare and publish a Fallow release with version, changelog, generated contracts, companion repositories, registry publication, and post-release verification.
---

# Release

## Preflight

1. Read `docs/development/quality-gates.md`, the release row in
   `docs/development/task-context-map.md`, and
   `docs/development/release-security.md`.
2. Confirm `main` is clean, current with `origin/main`, reviewed, and green.
   Check that the planned version tag is absent remotely and that no unresolved
   draft release or concurrent release is in progress.
3. Derive the semantic-version bump from every commit since the prior release
   unless the user supplied an explicit bump. Confirm a major bump before
   mutating versions unless the user explicitly requested it.
4. Dispatch the reusable release-validation workflow against `main` and require
   a completed successful result before changing versions or creating a tag.

## Prepare

5. Verify the complete `[Unreleased]` changelog against the commit range,
   issues, discussions, and external contributors since the prior tag. Ground
   public names, flags, rule IDs, and contributor handles in source or GitHub,
   not memory.
6. Draft curated public GitHub release notes before pushing the release tag.
   They must:

   - explain user-visible value rather than repeat commit subjects;
   - cover every material changelog entry and any breaking or migration detail;
   - use only public, generic examples;
   - verify contributor attribution;
   - end with the exact full-changelog comparison URL.

   Store the notes outside the repository, for example
   `/tmp/fallow-release-vX.Y.Z.md`, and require the file to be non-empty.
7. Apply version changes transactionally. Regenerate every public contract,
   adapter, packaged skill, and version-bearing artifact. Synchronize
   `fallow-docs` and `fallow-skills` from their canonical sources.
8. Run package dry-runs, generated-contract checks, companion-repository
   checks, and the repository's full release gates. Review the exact staged
   paths before creating a signed release commit and signed version tag.

## Publish

9. Push the release commit to `main`, then push the signed version tag. Create
   the curated GitHub Release immediately so the tag-triggered workflow can
   only upload assets to an already-documented release:

   ```bash
   # Use the verified values established during release preparation.
   TAG="v${VERSION}"
   TITLE="${TAG}: ${SUMMARY}"
   NOTES_FILE="/tmp/fallow-release-${TAG}.md"

   test -s "$NOTES_FILE"
   grep -qF "https://github.com/fallow-rs/fallow/compare/${PREVIOUS_TAG}...${TAG}" "$NOTES_FILE"

   git push origin main
   git push origin "$TAG"

   if gh release view "$TAG" --repo fallow-rs/fallow >/dev/null 2>&1; then
     gh release edit "$TAG" --repo fallow-rs/fallow \
       --title "$TITLE" --notes-file "$NOTES_FILE"
   else
     gh release create "$TAG" --repo fallow-rs/fallow --verify-tag \
       --title "$TITLE" --notes-file "$NOTES_FILE"
   fi
   ```

   The release workflow deliberately keeps `generate_release_notes: false`.
   It fails when the curated release, title, body, or comparison link is
   missing. Do not treat generated commit lists as curated release notes.
10. Update the rolling Action tags from maintainer credentials. Monitor the
    release workflow through `status=completed` and `conclusion=success`; a
    successful watch command alone is not sufficient evidence.

## Verify and close

11. Query the GitHub Release and require a non-draft, non-prerelease release
    with the expected title, a non-empty body, the exact comparison link, and
    uploaded assets. Then verify every published crate, npm package, editor
    package, binary, schema, documentation deployment, and companion contract
    from its real public endpoint.
12. Complete the required post-publication NAPI, Docker, rolling-tag, issue,
    discussion, and companion-repository follow-ups. Require their resulting
    `main` workflows to finish green and every touched worktree to be clean.

Do not report a release complete while any publication, public release-note,
registry, companion, deployment, or post-release verification gate is pending.
