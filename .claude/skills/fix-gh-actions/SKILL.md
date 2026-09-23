---
name: fix-gh-actions
description: Investigate and fix failing GitHub Actions checks for this repo or its PRs. Use when the user asks to debug CI failures, broken workflows, failed release jobs, or failing GitHub checks.
---
<!-- Generated from .agents/skills. Do not edit. -->

# Fix GitHub Actions

Use GitHub metadata, local repo inspection, and local reproduction where possible.

Workflow:
1. Identify the failing workflow, job, and step.
2. Classify the failure as flake, infra, or real before any action. When many
   unrelated jobs fail at once, check https://www.githubstatus.com first.
3. Before a retry, check the base with
   `git merge-base --is-ancestor origin/main HEAD`. On a stale base, rebase
   instead of a retry.
4. Retry a suspected flake one time. A second identical failure is a defect.
5. Inspect the relevant workflow files, scripts, jq filters, and code paths.
6. Reproduce locally when feasible.
7. Fix the smallest root cause rather than papering over symptoms.
8. Re-run targeted validation and report residual risk.

If the failure is in `action/` or `action.yml`, also use `github-action-review`.
