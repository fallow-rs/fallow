---
name: fix-gh-actions
description: Investigate and fix failing GitHub Actions checks for this repo or its PRs. Use when the user asks to debug CI failures, broken workflows, failed release jobs, or failing GitHub checks.
---
<!-- Generated from .agents/skills. Do not edit. -->

# Fix GitHub Actions

Use GitHub metadata, local repo inspection, and local reproduction where possible.

Workflow:
1. Identify the failing workflow, job, and step.
2. Inspect the relevant workflow files, scripts, jq filters, and code paths.
3. Reproduce locally when feasible.
4. Fix the smallest root cause rather than papering over symptoms.
5. Re-run targeted validation and report residual risk.

If the failure is in `action/` or `action.yml`, also use `github-action-review`.
