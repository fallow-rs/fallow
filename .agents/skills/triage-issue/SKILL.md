---
name: triage-issue
description: Triage a GitHub issue against the current fallow codebase and determine validity, priority, scope, and likely implementation shape. Use when the user asks to triage an issue, assess a bug report, or evaluate an enhancement request.
---

# Triage Issue

Use the GitHub connector and local repo inspection together.

Workflow:
1. Fetch the issue and comments.
2. Reproduce the report on a binary built from current `main` before you
   judge it. A report against an older release can be fixed already.
3. Inspect the relevant code paths locally.
4. Determine whether the issue is valid, overstated, underspecified, or already covered.
5. Recommend priority and whether the issue should be kept, split, narrowed, or closed.
6. Give acceptance criteria tied to the current codebase.
7. Add the `type:agent-ux` label when a coding agent misread the fallow
   output, a flag, the docs or an MCP tool result. The issue form field
   "How did you run fallow?" often shows this. Name the unclear part of the
   contract in the acceptance criteria.
8. Set the issue status. The repository board groups issues by status.
   - `Todo`: valid, actionable, and `priority:high` or `priority:medium`.
   - `Backlog`: valid but `priority:low`, a question, a tracking issue,
     `status:blocked`, or `status:major-version`.
   - `Not planned` or `Duplicate`: when you close the issue.
   - Do not set `In progress` or `In review`. A linked pull request sets them.

   `gh issue edit` has no status flag. Use GraphQL:

   ```bash
   gh api graphql -f query='{repository(owner:"fallow-rs",name:"fallow"){
     planningStatuses(first:20){nodes{id name}} issue(number:N){id}}}'
   gh api graphql -f query='mutation($i:ID!,$s:ID!){
     setIssueStatus(input:{issueId:$i,statusId:$s}){issue{status{name}}}}' \
     -f i=ISSUE_ID -f s=STATUS_ID
   ```

Give each claim about cause or history one confidence level:
- direct: you ran it, or the source shows it;
- supported: two independent sources agree;
- inferred: it follows from the evidence, but nothing shows it;
- speculative: plausible, with no evidence;
- unknown.

Write "because" only with a source: a `file:line`, a commit, or command
output. Code does not prove the intent of its author.

When useful, pull in:
- `rust-review`
- `panel-review`
- `team-assembly`

Always cite local file references for implementation claims.
