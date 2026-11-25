# Issue status

Each open issue in `fallow-rs/fallow` has a status. The shared "Triage board"
view of the repository groups open issues in columns by status. Maintainers
and contributors read the board to see which work is free and which work has
an owner.

## Statuses

| Status | Meaning | Who sets it |
|---|---|---|
| `Backlog` | Valid, but not next: `priority:low`, a question, a tracking issue, `status:blocked`, or `status:major-version`. | `triage-issue`, `sweep` |
| `Todo` | Valid, actionable, and `priority:high` or `priority:medium`. Nobody works on it. | `triage-issue`, `sweep` |
| `In progress` | Somebody works on it. | `implement`; GitHub for a linked draft pull request |
| `In review` | A linked pull request is open. | GitHub |
| `Done` | The issue is closed as completed. | GitHub |
| `Not planned` | The issue is closed as not planned. | `triage-issue` |
| `Duplicate` | The issue is closed as a duplicate. | `triage-issue` |

GitHub sets a status only for a linked pull request. A pull request links
to an issue through a closing keyword such as `Closes #N`. A pull request
with `Refs #N` has no link, so GitHub does not change the status of the
issue. The skill that does the work then sets the status.

A status label such as `status:blocked` stays a label. The status set of
GitHub is fixed and has no value for it.

## Commands

`gh issue edit` and `gh issue create` have no status flag. Use GraphQL.

Read the status IDs and the node ID of an issue:

```bash
gh api graphql -f query='{repository(owner:"fallow-rs",name:"fallow"){
  planningStatuses(first:20){nodes{id name}} issue(number:N){id status{name}}}}'
```

Set the status of an issue:

```bash
gh api graphql -f query='mutation($i:ID!,$s:ID!){
  setIssueStatus(input:{issueId:$i,statusId:$s}){issue{status{name}}}}' \
  -f i=ISSUE_ID -f s=STATUS_ID
```

Find issues that somebody works on:

```bash
gh issue list --search 'status:"In progress"'
```

The search needs the exact status name in quotes.
