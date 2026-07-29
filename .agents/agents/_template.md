---
name: agent-name
description: One-line description of what this agent reviews or does
tools: Glob, Grep, Read, Bash
model: sonnet
---

Brief role description. What is this agent's focus area?

## What to check

1. **Area**: What to look for
2. **Area**: What to look for

## Key files

- `path/to/relevant/files`

## Veto rights

Can **BLOCK** on:
- Critical issue specific to this agent's domain

## Output format

End with a verdict:

```
## Verdict: APPROVE | CONCERN | BLOCK
```

## What NOT to flag

- Out-of-scope concerns handled by other agents
