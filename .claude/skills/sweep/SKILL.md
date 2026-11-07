---
name: sweep
description: Audit the current Fallow session for missed work, incomplete verification, stale documentation, companion drift, or cleanup before final completion.
---
<!-- Generated from .agents/skills. Do not edit. -->

# Sweep

In this repository, use this sweep instead of a general sweep workflow. Run it
at the end of every objective, without a prompt from the maintainer.

1. Re-read the user's full request, active plan, diff, and live pull-request
   state.
2. Map every acceptance criterion to authoritative evidence.
3. Check for missed consumers, output formats, filters, docs, skills, schemas,
   CI scripts, companion repositories, and private/public boundaries.
4. Re-run any weak or stale verification.
5. Fix each in-scope omission now. A fix after the merge is a new pull
   request: send it through `review` and `ship`, and add it to the list of
   step 6. Sort each separate follow-up into the three groups of
   `CONTRIBUTING.md`. Add an improvement idea with no known need to the
   "Possible follow-ups" section of the PR body, also after the merge, and
   open no issue for it. Track each item of needed work:
   - in a `fallow-rs` repository, open a GitHub issue only for work that is
     known to be needed. Write it for the maintainer as the reader, not about
     them. Put an open decision for the maintainer in the final report, with
     a recommended answer, and open no issue for it;
   - do not propose to move release-only checks (the full Windows suite,
     arm64, the drift harness) into pull-request CI. They run only in
     release validation on purpose;
   - before you create a public issue, check the body against the trust
     boundary in `CLAUDE.md`. Put private context in the private repository;
   - for a repository outside `fallow-rs`, write a draft issue in the final
     report. The maintainer posts it.
   Link each issue or draft in the final report. A note in `.plans/`, in
   `/tmp`, or in the chat does not track a follow-up.
6. Check each pull request in the list of `.plans/<task>.md` with
   `gh pr view <url> --json state,mergeCommit`. For each merge commit, run
   `node scripts/ship-wait-checks.mjs --commit <sha> --follow-main` (for a
   companion repository, use the commands of the ship skill). Each pull
   request is merged with green merged-commit CI, or goes in the report as
   blocked, with the blocker and the next action. Armed auto-merge does not
   pass this check.
7. Verify branches, worktrees, temporary worktrees, and generated artifacts are
   clean.
8. List the lessons of the task: corrections, failures that cost time, and
   skipped steps. Keep a lesson only when all four filters pass:
   - it stays true after paths, versions, and counts change;
   - it changes a future decision, not only the amount of text to read;
   - no skill or doc covers it yet (sharpen a weak rule, do not copy it);
   - it belongs to a skill, doc, or surface that this task used.
9. Store each kept lesson in the strongest form: a type or schema, then a
   lint, test, or CI check, then a script. Write text only when none fits.

Finding no obvious bug is not completion. Prove every requested outcome.
