---
name: ship
description: Land completed Fallow work after review, run pre-push parity, monitor merged-commit CI, and leave repositories clean. Use when asked to ship, merge, or move approved work to main.
---

# Ship

1. Confirm the branch and diff exactly match the reviewed scope.
2. Re-run the required checks from `docs/development/quality-gates.md`.
3. Verify that public contracts and generated files are pushed. Merge each
   companion change into the default branch of its repository (for example
   fallow-docs `main`). A pushed feature branch is not enough: the release
   then must merge it.
4. Create signed conventional commits only.
5. Merge through the repository's current protected-main workflow.
6. Monitor the merged commit until required CI completes. Use
   `node scripts/ship-wait-checks.mjs --commit <full sha> --min-checks <count> --follow-main`,
   with the run count of a recent push to `main`. A newer push to `main`
   cancels the runs of the merged commit. With `--follow-main`, the script
   then waits for the newest commit of `origin/main` that contains the
   merged commit, and prints one line for each move. Before you retry a
   failed job, confirm that the branch contains the current base
   (`git merge-base --is-ancestor origin/main HEAD`). A second identical
   failure is a defect, not a flake: diagnose it instead of a third retry.
7. Inspect the merged tree for conflict markers and generated drift.
8. Return every touched checkout to a clean, synchronized state.

Do not call work complete while required merged-commit workflows are still
running.

## Serial merge of many pull requests

Each merge changes `CHANGELOG.md` on `main`, so the next branch conflicts.
Use these scripts for each pull request, one at a time:

1. Rebase the branch: `node scripts/ship-rebase.mjs --pr <number> --push`.
   The script keeps both sides of a `CHANGELOG.md` conflict and stops on
   any other conflict. Git rerere is off for the rebase. The script pushes
   only when these checks pass:
   - Outside `CHANGELOG.md`, the result is the same as a merge of the
     branch into `main`. A rebase drops the changes that a merge commit of
     the branch made, so this check finds them. At a path where the merge
     itself conflicts, the check has no reference. That path fails the
     check only when the branch has merge commits. Then compare the path
     by hand. The failure does not always mean a lost change.
   - The branch adds the same `CHANGELOG.md` lines as before the rebase,
     and the rebase removes no line of `main`.
   - Each entry that the branch adds to the first release section is still
     in that section. A release on `main` puts a version heading under
     `## [Unreleased]`, and an entry of the branch can land under it.
     Then the check fails and lists the entries. HEAD holds the rebased
     branch. A fix on the old branch does not help, because the next run
     rebases it again and moves the entries again. After a push of HEAD,
     the next run compares the pushed result with itself, so it cannot
     find a problem of the first rebase or of the manual fix. When other
     checks also fail, the script prints no push command: fix the other
     problems first. When the moved entries are the only problem, the
     script prints these steps. Do them on HEAD:
     1. Move the entries to the first release section of `CHANGELOG.md`
        and commit the change.
     2. Make sure that the printed `git diff <rebased tip> HEAD` command
        shows only the moved entries. This is the only check of the
        manual fix.
     3. Push HEAD with the lease command that the script prints, for
        example
        `git push --force-with-lease=refs/heads/<branch>:<old tip> origin HEAD:refs/heads/<branch>`.
     4. Run the script again. It confirms that the branch is on `main`.
        It cannot check the first rebase again.
   - The rebase adds no second `###` subsection with the same name to the
     first release section.
   - Each version bump of the branch is still a change against `main`.
     This covers each `*CACHE_VERSION` constant, for example
     `GRAPH_CACHE_VERSION`, and each `*SCHEMA_VERSION` constant, for
     example `CHECK_SCHEMA_VERSION`.
2. Wait for CI. Set `--min-checks` to the check count of a recent complete
   run. Without this guard, a read right after the push can show no
   pending check.

   ```bash
   node scripts/ship-wait-checks.mjs --pr <number> --min-checks <count>
   ```

   GitHub starts no `pull_request` workflow for a pull request that
   conflicts with its base. The script then stops with exit code 3.
   Update the branch, push, and wait again.

3. Merge through the protected-main workflow. Then continue with the next
   pull request.

For a companion pull request in fallow-rs/docs, run
`node <fallow>/scripts/ship-docs-rebase.mjs --pr <number> --push` in a
docs checkout with no untracked files. The manifest generator reads the
directory, so the script refuses untracked files. The script regenerates
`public-content-manifest.json` for each commit that conflicts on it. It
runs the same merge comparison outside that file. Then add
`--repo fallow-rs/docs` to the wait command.

The rebase scripts work on a detached HEAD. If a script stops on a
conflict, the rebase stays in progress: resolve it by hand, or run
`git rebase --abort`. If a check fails, the script does not push. Each
script prints its options and exit codes with `--help`.
