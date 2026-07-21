---
paths:
  - "**/*.rs"
  - "**/*.js"
  - "**/*.jsx"
  - "**/*.mjs"
  - "**/*.cjs"
  - "**/*.ts"
  - "**/*.tsx"
  - "**/*.py"
  - "**/*.sh"
  - "**/*.yml"
  - "**/*.yaml"
  - "**/*.toml"
  - "**/*.css"
  - "**/*.scss"
  - "**/*.vue"
  - "**/*.svelte"
  - "**/*.astro"
---

# Code comments

- Do not add inline comments that narrate routine steps, label standard control flow, or restate adjacent code.
- Inline comments should preserve non-obvious rationale, invariants, safety constraints, protocol behavior, performance tradeoffs, compatibility workarounds, or issue context.
- Rustdoc and JSDoc are API documentation. They may describe public behavior, contracts, examples, errors, and safety requirements.
- Before finishing, inspect every added or changed comment. Remove narration and generic scaffolding.
