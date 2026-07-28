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
   Require repository release immutability to be enabled. Check that the
   planned version tag is absent remotely and that no unresolved draft release
   or concurrent release is in progress.
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
6. Draft curated public GitHub release notes before starting the publication
   workflow. They must:

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
   paths before creating a signed release commit. Do not create the version tag
   yet.

## Publish

9. Push the release commit to `main`. While the version tag is still absent,
   dispatch the release workflow from `main` with the planned tag:

   ```bash
   # Use the verified values established during release preparation.
   TAG="v${VERSION}"
   TITLE="${TAG}: ${SUMMARY}"
   NOTES_FILE="/tmp/fallow-release-${TAG}.md"
   RELEASE_COMMIT="$(git rev-parse HEAD)"

   test -s "$NOTES_FILE"
   grep -qF "https://github.com/fallow-rs/fallow/compare/${PREVIOUS_TAG}...${TAG}" "$NOTES_FILE"

   git push origin main
   git fetch origin main
   if [ "$(git rev-parse origin/main)" != "$RELEASE_COMMIT" ]; then
     echo "origin/main moved away from the prepared release commit" >&2
     exit 1
   fi
   if git ls-remote --exit-code --tags origin "refs/tags/${TAG}" >/dev/null 2>&1; then
     echo "release tag already exists: ${TAG}" >&2
     exit 1
   fi

   gh workflow run release.yml --ref main \
     -f tag="$TAG"
   ```

   The workflow deliberately has no tag trigger and never creates a tag or
   GitHub Release. It validates and builds the release, stores the complete
   flattened GitHub asset bundle as the `release-assets` Actions artifact, and
   publishes registries while the tag remains absent.
10. Monitor the specific workflow run through `status=completed` and
    `conclusion=success`; a successful watch command alone is not sufficient
    evidence. Require the `Release ready for signed tag` job to pass, then
    verify every registry and marketplace publication is live.
    Download the `release-assets` artifact from that exact run and confirm it
    is non-empty. Only then create and push the signed tag and create the
    immutable GitHub Release:

    ```bash
    ASSET_DIR="$(mktemp -d)"
    gh run download "$RUN_ID" \
      --name release-assets \
      --dir "$ASSET_DIR"
    test -n "$(find "$ASSET_DIR" -maxdepth 1 -type f -print -quit)"

    if git ls-remote --exit-code --tags origin "refs/tags/${TAG}" >/dev/null 2>&1; then
      echo "release tag appeared before publication completed: ${TAG}" >&2
      exit 1
    fi

    git tag -s "$TAG" "$RELEASE_COMMIT" -m "Fallow ${VERSION}"
    git verify-tag "$TAG"
    git push origin "$TAG"

    gh release create "$TAG" "$ASSET_DIR"/* \
      --repo fallow-rs/fallow \
      --verify-tag \
      --title "$TITLE" \
      --notes-file "$NOTES_FILE"
    ```

    GitHub CLI creates a draft internally, uploads every asset, and only then
    publishes it, matching GitHub's immutable-release guidance. If the workflow
    fails, repair and rerun it without burning a tag. If release creation fails
    after tag push, keep the signed tag, remove only an incomplete draft if one
    exists, and retry release creation. Never recreate or move the signed tag.
    Update the rolling Action tags from maintainer credentials only after the
    immutable release is published.

## Verify and close

11. Query the GitHub Release and require a non-draft, non-prerelease release
    marked immutable, with the expected title, a non-empty body, the exact
    comparison link, the signed tag at `RELEASE_COMMIT`, and uploaded assets.
    Then verify every published crate, npm package, editor package, binary,
    schema, documentation deployment, and companion contract from its real
    public endpoint.
12. Complete the required post-publication NAPI, Docker, rolling-tag, issue,
    discussion, and companion-repository follow-ups. Require their resulting
    `main` workflows to finish green and every touched worktree to be clean.

Do not report a release complete while any publication, public release-note,
registry, companion, deployment, or post-release verification gate is pending.
