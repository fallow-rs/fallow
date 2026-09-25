# Release procedure

Maintainer-only procedure for publishing a Fallow release: version, changelog,
generated contracts, companion repositories, registry publication, and
post-release verification. The policy tests under `scripts/` assert the
invariants in this file.

## Preflight

1. Read `docs/development/quality-gates.md`, the release row in
   `docs/development/task-context-map.md`, and
   `docs/development/release-security.md`.
2. Confirm `main` is clean, current with `origin/main`, reviewed, and green.
   Check that the planned version tag is absent remotely and that no
   unresolved draft release or concurrent release is in progress.
3. Require repository release immutability to be enabled. The release workflow
   cannot check this: the endpoint needs the Administration read permission,
   which is not a grantable workflow token scope. Verify it here, with
   maintainer credentials, as the only gate:

   ```bash
   enabled="$(gh api -H "X-GitHub-Api-Version: 2026-03-10" \
     repos/fallow-rs/fallow/immutable-releases --jq '.enabled')"
   [ "$enabled" = "true" ] || { echo "Release immutability is not enabled" >&2; exit 1; }
   ```

   Require the `release` environment to admit `main` only, and check where each
   publication secret lives. GitHub never returns a secret value, so a secret
   moves into the environment only when its value is entered again, which in
   practice means at its next rotation. Until then it stays a repository
   secret, readable by a workflow on any ref, and the environment does not
   protect it: the check names those secrets instead of failing. Once a secret
   is in the environment the repository copy must be gone, and the check fails
   on a secret that exists at both levels or at neither:

   ```bash
   custom="$(gh api repos/fallow-rs/fallow/environments/release \
     --jq '.deployment_branch_policy.custom_branch_policies')"
   [ "$custom" = "true" ] || { echo "release environment has no custom branch policy" >&2; exit 1; }
   branches="$(gh api repos/fallow-rs/fallow/environments/release/deployment-branch-policies \
     --jq '[.branch_policies[].name] | join(",")')"
   [ "$branches" = "main" ] || { echo "release environment admits: ${branches:-nothing}" >&2; exit 1; }
   repo_secrets="$(gh secret list --repo fallow-rs/fallow --json name --jq '.[].name')"
   env_secrets="$(gh secret list --repo fallow-rs/fallow --env release --json name --jq '.[].name')"
   for name in VSCE_PAT OVSX_PAT ED25519_BINARY_SIGNING_PRIVATE_KEY; do
     in_repo=0; in_env=0
     grep -qx "$name" <<<"$repo_secrets" && in_repo=1
     grep -qx "$name" <<<"$env_secrets" && in_env=1
     case "${in_env}${in_repo}" in
       10) ;;
       11) echo "$name exists at both levels; delete the repository copy" >&2; exit 1 ;;
       01) echo "NOTE: $name is a repository secret, unprotected by the release environment; move it at its next rotation" >&2 ;;
       *) echo "$name is missing" >&2; exit 1 ;;
     esac
   done
   ```
4. Derive the semantic-version bump from every commit since the prior release
   unless the user supplied an explicit bump. Confirm a major bump before
   mutating versions unless the user explicitly requested it.
5. Dispatch the reusable release-validation workflow against `main` and require
   a completed successful result before changing versions or creating a tag.

## Prepare

6. Verify the complete `[Unreleased]` changelog against the commit range,
   issues, discussions, and external contributors since the prior tag. Ground
   public names, flags, rule IDs, and contributor handles in source or GitHub,
   not memory.
7. Draft curated public GitHub release notes before starting the publication
   workflow. They must:

   - explain user-visible value rather than repeat commit subjects;
   - cover every material changelog entry and any breaking or migration detail;
   - use only public, generic examples;
   - name no competing or upstream third-party project;
   - contain no em-dash characters in the title or the body;
   - verify contributor attribution;
   - end with the exact full-changelog comparison URL.

   The published release is immutable, so every one of these is a
   pre-publication gate, not something to repair afterwards.

   Store the notes outside the repository, for example
   `/tmp/fallow-release-vX.Y.Z.md`, and require the file to be non-empty.

   `scripts/verify-release-metadata.mjs` enforces the machine-checkable half of
   this list, so skipping the runbook no longer skips the gates. Two surfaces,
   because the notes and the release exist at different moments:

   - `release.yml` runs it at dispatch against `CHANGELOG.md`, the source the
     notes are drafted from. It requires one dated `## [X.Y.Z]` section with a
     non-empty body and no em-dash, the matching compare-link definition back
     to the previous released version, and an empty `[Unreleased]` section so
     nothing ships uncredited. Publication cannot start without it.
   - `release-published.yml` runs it again on the `release: published` event,
     the first moment a title and body exist, against the title prefix, the
     non-empty body, the comparison URL, em-dashes, and third-party names.
     It reads the default branch, which keeps moving while a release
     publishes, so it re-checks only the released section, which is frozen.
     The empty-`[Unreleased]` rule is a dispatch-time rule: anything merged
     during the publish would otherwise fail a correct release.

   The changelog gate deliberately omits the third-party-name rule: changelog
   entries legitimately name a migration source or a documented parity gap,
   while the public release surface does not.
8. Apply version changes transactionally. Regenerate every public contract,
   adapter, packaged skill, and version-bearing artifact. Synchronize
   `fallow-docs` and `fallow-skills` from their canonical sources.

   Two version-bearing manifests sit outside the workspace bump and follow
   opposite rules. `scripts/sync-npm-versions.sh` rewrites both and is wired to
   no workflow, so a release can skip it silently.

   - `tools/type-aware-sidecar` bumps in lockstep with the CLI, inside the
     release commit. The CLI refuses a companion whose version differs from the
     binary, so a skipped bump turns `main` red on Windows validation rather
     than at publish time.
   - `crates/napi` and its `@fallow/*` platform manifests stay at the last
     published version through the release commit, keeping any dependabot bump
     that landed on top. Their platform packages do not exist on npm at the new
     version until publish runs, so bumping them early leaves a lockfile with
     unresolvable entries and `npm ci` fails. They sync in the post-publish
     catch-up step.

   Version-string assertions do not catch the second case. Verify a touched
   lockfile with `cd crates/napi && rm -rf node_modules && npm ci`.

   The comparative performance numbers are the one artifact a version bump
   cannot regenerate: `benchmarks/bench-ci.sh` runs only fallow, and
   `benchmarks/compare.mjs` is the only script that also runs knip. Refresh
   them deliberately, not every release:

   - Re-run `node benchmarks/compare.mjs` on the same class of hardware named
     in the `BENCHMARKS.md` environment line, so the numbers stay comparable
     with the ones they replace.
   - Update the Reference Results tables, the environment line, and the README
     Performance paragraph in the same commit. `scripts/repository-policy.test.mjs`
     fails when the README quotes a number or a measurement vintage that the
     environment line no longer carries.

   A release is never blocked on a re-measurement. A stale but internally
   consistent capture is correct; a README that quotes numbers no benchmark
   run produced is not.

   One more constant is version-bearing and no generator touches it:
   `RELEASED_TAG` in `scripts/repository-policy.test.mjs`. It names the
   published release the required-field schema policy compares against. Set it
   to the tag just published, in the first commit after the release. Left
   stale, the policy still runs but weakens: an envelope that bumped in an
   earlier release satisfies it for a field dropped in a later one.
9. Run package dry-runs, generated-contract checks, companion-repository
   checks, and the repository's full release gates. Review the exact staged
   paths before creating a signed release commit with the subject
   `chore: release vX.Y.Z`. The release workflow checks this subject: the
   push workflows exempt it from cancellation, and the CI gate needs its runs.
   Do not create the version tag yet.

## Publish

10. Push the release commit to `main`. While the version tag is still absent,
    dispatch the release workflow from `main` with the planned tag:

    ```bash
    # Use the verified values established during release preparation.
    TAG="v${VERSION}"
    TITLE="${TAG}: ${SUMMARY}"
    NOTES_FILE="/tmp/fallow-release-${TAG}.md"
    RELEASE_COMMIT="$(git rev-parse HEAD)"

    test -s "$NOTES_FILE"
    grep -qF "https://github.com/fallow-rs/fallow/compare/${PREVIOUS_TAG}...${TAG}" "$NOTES_FILE"

    # release.yml gates the changelog section these notes are drafted from, but
    # the release does not exist while it runs, so the title and the notes file
    # are checked here, before the dispatch that publishes every registry.
    case "$TITLE" in
     "${TAG}: "?*) ;;
     *) echo "release title must be '${TAG}: <summary>'" >&2; exit 1 ;;
    esac
    EM_DASH="$(printf '\xe2\x80\x94')"
    if printf '%s' "$TITLE" | grep -qF "$EM_DASH" || grep -qF "$EM_DASH" "$NOTES_FILE"; then
     echo "release title or notes contain an em-dash" >&2
     exit 1
    fi

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

    The first job, `release-context`, has a CI gate. It runs
    `scripts/verify-release-ci.mjs` against the release commit, and nothing
    builds or publishes before it passes. Pull requests run only part of CI,
    and the other checks run on push to `main`. Thus the gate waits for the
    push runs on the release commit, for up to 150 min. It fails when:

    - a workflow in `REQUIRED_WORKFLOWS` has no run on the release commit;
    - a workflow in `REQUIRED_WORKFLOWS` did not end with success;
    - any other push run on the release commit failed, was cancelled, or
      timed out;
    - a run is still queued or in progress after the timeout.

    Each problem prints one `::error::` line with the fix:

    - A missing run: a path filter did not match the release commit, or the
      run did not start. When the workflow has a `workflow_dispatch`
      trigger, and `origin/main` is still the release commit, start it with
      `gh workflow run <file> --ref main`. Then dispatch the release again.
      `ci.yml` and `commitlint.yml` have no `workflow_dispatch` trigger. For
      these, push a new commit to `main` and prepare the release from that
      commit. The error line tells you which fix applies.
    - A failed run: open the run from the error line. For a flaky failure,
      re-run it with `gh run rerun <id> --failed`. The gate reads the latest
      attempt. For a real failure, fix it in a new commit on `main`, and
      prepare the release from that commit.
    - A cancelled run: on `main`, a newer push cancels an older push run of
      the same workflow. The release commit is the exception: a commit whose
      message starts with `chore: release v` gets a concurrency group of its
      own, so a later merge cannot cancel its runs. When the release commit
      has another message, its runs can be cancelled. Re-run a cancelled run
      with `gh run rerun <id>`, or prepare the release again with the correct
      commit message.
    - A timeout: the runner queue was too long. Wait until the runs finish,
      then dispatch the release again. The runs of the release commit keep
      their state.

    When you add a workflow that runs on push to `main`, or move a check from
    pull requests to `main`, update `REQUIRED_WORKFLOWS` in
    `scripts/verify-release-ci.mjs`.

    The workflow deliberately has no tag trigger and never creates a tag or
    GitHub Release. It validates and builds the release, stores the complete
    flattened GitHub asset bundle as the `release-assets` Actions artifact, and
    publishes registries while the tag remains absent. The `fallow` npm root is
    the exception: the workflow stages it and step 11 approves it. The VS Code release is
    published by separate Marketplace and Open VSX jobs. A credential-free
    public verifier checks every exact target before the final release gate.
11. The run pauses once, in `Wait for the approved fallow root`, and the
    maintainer approval is what lets it continue. Monitor the run until the
    `Publish to npm` job has completed successfully; the crates publish in
    parallel and the wait job is then running.

    The workflow stages the `fallow` npm root instead of publishing it. Its
    trusted publisher grants stage publish only, so the version becomes
    installable only when the maintainer approves the stage with npm 2FA. The
    VSIX publishers wait for that approval because the VS Code extension
    downloads its binary from the GitHub Release of its own version, and that
    release is created after the approval. Before approving, prove that the
    staged bytes are the tarball this exact run built:

    ```bash
    STAGE_LIST="$(npm stage list fallow)"
    test "$(grep -c '^id: ' <<<"$STAGE_LIST")" -eq 1
    grep -qx "version: ${VERSION}" <<<"$STAGE_LIST"
    STAGE_ID="$(sed -n 's/^id: //p' <<<"$STAGE_LIST")"

    NPM_DIR="$(mktemp -d)"
    STAGE_DIR="$(mktemp -d)"
    gh run download "$RUN_ID" --name npm-tarballs --dir "$NPM_DIR"
    (cd "$STAGE_DIR" && npm stage download "$STAGE_ID")
    BUILT="$(shasum -a 256 < "$NPM_DIR/20-cli-root/fallow-${VERSION}.tgz")"
    STAGED="$(shasum -a 256 < "$STAGE_DIR/fallow-${VERSION}-${STAGE_ID}.tgz")"
    test "$BUILT" = "$STAGED"

    npm stage approve "$STAGE_ID"   # interactive npm 2FA
    test "$(npm view "fallow@${VERSION}" version)" = "$VERSION"
    test "$(npm view "fallow@${VERSION}" dist.attestations.provenance.predicateType)" \
      = "https://slsa.dev/provenance/v1"
    ```

    A digest mismatch, a second stage, or a stage at another version means the
    stage did not come from this run: reject it with `npm stage reject` and
    investigate before anything else. A rejected version can be staged again, so
    recovery is a rerun of the `Publish to npm` job, which skips every package
    that already landed. No tag exists yet, so nothing is burned.

    After the approval the wait job downloads the public tarball, requires its
    sha256 to equal the digest `Publish to npm` recorded, and releases both VSIX
    publishers; a public tarball with other bytes fails the run instead. The
    wait gives up after a bounded time with the recovery in its message:
    approve, then rerun the failed jobs of the same run. Monitor the run through
    `status=completed` and `conclusion=success`; a successful watch command
    alone is not sufficient evidence. Require `similar-code-conformance` to have
    validated the exact Linux x64 sidecar artifact against the committed F32
    Candle baseline before `release-verified`, and require the `Publish VS Code Marketplace
    targets`, `Publish Open VSX targets`, `Verify public VS Code registry
    targets`, and `Release ready for signed tag` jobs to pass. The
    public verifier requires the exact universal plus six platform tuples and
    normalized payloads from both registries, without accepting a universal
    fallback.

    Download the `release-assets` artifact from that exact run and confirm it
    is non-empty. Confirm it contains the seven target VSIX files,
    `inventory.json`, and `SHA256SUMS`. Only then create and push the signed tag
    and create the immutable GitHub Release. Do this right after
    `release-ready` without other work in between: from the moment the VSIX is
    public until the GitHub Release exists, an extension that updates cannot
    fetch its binary.

    ```bash
    ASSET_DIR="$(mktemp -d)"
    gh run download "$RUN_ID" \
      --name release-assets \
      --dir "$ASSET_DIR"
    test -n "$(find "$ASSET_DIR" -maxdepth 1 -type f -print -quit)"
    test "$(find "$ASSET_DIR" -maxdepth 1 -name 'fallow-vscode-*.vsix' -type f | wc -l | tr -d ' ')" -eq 7
    test -f "$ASSET_DIR/inventory.json"
    test -f "$ASSET_DIR/SHA256SUMS"

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

12. Query the GitHub Release and require a non-draft, non-prerelease release
    marked immutable, with the expected title, a non-empty body, the exact
    comparison link, the signed tag at `RELEASE_COMMIT`, and uploaded assets.
    Publication also triggers `release-published.yml`, which asserts the same
    title, body, comparison link, and naming rules. Require that run green.
    Assets, tag and target are frozen at publication, but the title and body
    are not, so a failure there is repaired with `gh release edit --title` or
    `--notes-file` rather than by recreating anything.
    Then verify every published crate, npm package, editor package, binary,
    schema, documentation deployment, and companion contract from its real
    public endpoint.
13. Complete the required post-publication NAPI, Docker, rolling-tag, issue,
    discussion, and companion-repository follow-ups. Require their resulting
    `main` workflows to finish green and every touched worktree to be clean.

Do not report a release complete while any publication, public release-note,
registry, companion, deployment, or post-release verification gate is pending.
