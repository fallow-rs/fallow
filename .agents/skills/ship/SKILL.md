---
name: ship
description: Land completed Fallow work after review, run pre-push parity, monitor merged-commit CI, and leave repositories clean. Use when asked to ship, merge, or move approved work to main.
---

# Ship

1. Confirm the branch and diff exactly match the reviewed scope. To put
   the branch on the current base, run
   `node scripts/ship-rebase.mjs --pr <number> --push`, also for one pull
   request. Do not rebase by hand. The script compares the result with a
   merge and checks each version constant. A failed manual
   `rebase --continue` leaves commits on a detached HEAD. For a stacked
   branch after a squash merge of its parent, add `--upstream` (see
   "Serial merge of many pull requests").
2. Re-run the required checks from `docs/development/quality-gates.md`.
3. Verify that public contracts and generated files are pushed. Merge each
   companion change into the default branch of its repository (for example
   fallow-docs `main`). A pushed feature branch is not enough: the release
   then must merge it. When the merged change touches `npm/fallow/skills`,
   run `node scripts/sync-skills-companion.mjs` and push the fallow-skills
   commit that it prepares.
4. Create signed conventional commits only.
5. Merge through the repository's current protected-main workflow.
6. Monitor the merged commit until required CI completes. Use
   `node scripts/ship-wait-checks.mjs --commit <full sha> --min-checks <count> --follow-main`,
   with the run count of a recent push to `main`. A newer push to `main`
   cancels the runs of the merged commit. With `--follow-main`, the script
   then waits for the newest commit of `origin/main` that contains the
   merged commit, and prints one line for each move. On a busy `main`, new
   pushes can cancel each long run, and the wait can time out with pending
   runs. Then do not start the same wait again. For each workflow without a
   pass, watch its newest in-progress run on a commit that contains the
   merged commit (`gh run watch <run id> --exit-status`). Before you retry a
   failed job, confirm that the branch contains the current base
   (`git merge-base --is-ancestor origin/main HEAD`). A second identical
   failure is a defect, not a flake: diagnose it instead of a third retry.
7. Inspect the merged tree for conflict markers and generated drift. Then
   run `git diff <reviewed head> <merge commit>`. A pull request that merged
   after your last rebase shows there, also when it did not conflict. Read
   each hunk in a file that your pull request changed. Such a merge can make
   your code, a comment or a doc wrong without a text conflict. Fix that in
   a follow-up pull request.
8. Return every touched checkout to a clean, synchronized state.

Do not call work complete while required merged-commit workflows are still
running.

## Serial merge of many pull requests

A pull request adds its changelog entries as a file in `changelog.d/`, so
merges do not conflict in the changelog. A branch from before `changelog.d/`
still edits `CHANGELOG.md`. Move its entries to a fragment
(`changelog.d/README.md`), because the `Changelog fragments` CI step fails it.
Use these scripts for each pull request, one at a time:

1. Rebase the branch: `node scripts/ship-rebase.mjs --pr <number> --push`.
   The script resolves the conflicts that have one mechanical answer and
   stops on any other conflict:
   - `CHANGELOG.md`: it keeps both sides.
   - A `*CACHE_VERSION` constant: it takes the value of `main`. After the
     rebase, a new commit sets each cache bump of the branch that the
     rebase lost to the value of `main` + 1.
   - A generated contract file: it takes the side of `main`. Then it does
     not push and prints the regeneration steps for the kept worktree.

   Git rerere is off for the rebase. The script pushes only when these
   checks pass:
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
     Then the check fails and lists the entries. The kept temporary
     worktree holds the rebased branch. A fix on the old branch does not
     help, because the next run rebases it again and moves the entries
     again. After a push of the result, the next run compares the pushed
     result with itself, so it cannot find a problem of the first rebase
     or of the manual fix. When other checks also fail, the script prints
     no push command: fix the other problems first. When the moved entries
     are the only problem, the script prints these steps. Do them in the
     kept worktree:
     1. Move the entries to the first release section of `CHANGELOG.md`
        and commit the change. Make sure that the printed
        `git -C <worktree> diff <rebased tip> HEAD` command shows only the
        moved entries. This is the only check of the manual fix.
     2. Push with the lease command that the script prints, for example
        `git -C <worktree> push --force-with-lease=refs/heads/<branch>:<old tip> origin HEAD:refs/heads/<branch>`.
     3. Remove the worktree with the printed
        `git -C <checkout> worktree remove --force <worktree>` command.
     4. Run the script again. It confirms that the branch is on `main`.
        It cannot check the first rebase again.
   - The rebase adds no second `###` subsection with the same name to the
     first release section.
   - Each version bump of the branch is still a change against `main`.
     This covers each `*SCHEMA_VERSION` constant, for example
     `CHECK_SCHEMA_VERSION`, and each `*CACHE_VERSION` constant that the
     script could not bump again (a string value or a moved constant).

   A stacked branch still holds the original commits of its parent branch.
   After a squash merge of the parent, a plain rebase replays those commits
   on the squash, and they conflict. Do not run `git rebase --onto` by
   hand: a manual resolution with `git checkout --ours` on a whole file
   drops each hunk of the branch in that file, also a hunk without a
   conflict. Give the old tip of the parent branch to the script instead:

   ```bash
   node scripts/ship-rebase.mjs --pr <number> --upstream <old parent tip> --push
   ```

   The old parent tip is the last commit of the parent branch in the
   history of the stacked branch. The script then runs
   `git rebase --onto <base> <old parent tip>` and replays only the own
   commits of the branch. The checks change in this way:
   - The tree check merges the branch into `main` with the old parent tip
     as the merge base, because `main` already holds the squash of the
     parent.
   - The hunk check compares the hunks of `<old parent tip>..<old tip>`
     with the hunks of `main..<new tip>`, without context lines. A lost
     or changed hunk fails the check, also when the merge comparison
     passes. The check leaves out `CHANGELOG.md`, the generated files that
     took the side of `main`, and the cache version lines. A failure
     prints the `git range-diff` command that compares the commits.
   - The `CHANGELOG.md` check and the version checks use the old parent
     tip as the old base. The cache version handling is the same: a lost
     cache bump gets the value of `main` + 1.

   The option needs Git 2.40 or later.
2. Wait for CI. Set `--min-checks` to the check count of a recent complete
   run. Without this guard, a read right after the push can show no
   pending check. Set `--head` to the pushed tip, which the rebase script
   prints. Right after a push, GitHub can still report the checks of the
   old head.

   ```bash
   node scripts/ship-wait-checks.mjs --pr <number> --head <full sha> --min-checks <count>
   ```

   GitHub starts no `pull_request` workflow for a pull request that
   conflicts with its base. The script then stops with exit code 3.
   Update the branch, push, and wait again.

3. Merge through the protected-main workflow. Then continue with the next
   pull request.

For a companion pull request in fallow-rs/docs, run
`node <fallow>/scripts/ship-docs-rebase.mjs --pr <number> --push` in a
docs checkout. The script regenerates `public-content-manifest.json` for
each commit that conflicts on it. The manifest generator reads the
directory. The rebase runs in a fresh worktree, so untracked files of the
checkout do not get into the manifest. The script runs the same merge
comparison outside that file. Then add `--repo fallow-rs/docs` to the
wait command.

The rebase scripts do not change the checkout where they run. They add a
temporary worktree with a detached HEAD, and the rebase, the checks and
the push run there. The checkout can be on any branch and can have
changes. The pre-push hook of the checkout runs on the push. When the
checks pass, the script removes the worktree. After a stop, a failed
check or a failed push, the script keeps the worktree and prints its
path and the next commands:

- After a conflict, the rebase stays in progress in the worktree.
  Resolve it there and run `git -C <worktree> rebase --continue`, or run
  `git -C <worktree> rebase --abort`.
- If a check fails, the script does not push.
- To discard the worktree, run the printed
  `git -C <checkout> worktree remove --force <worktree>` command.

Each script prints its options and exit codes with `--help`.
