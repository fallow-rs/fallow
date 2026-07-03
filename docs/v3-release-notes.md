# Fallow v3 Release Notes Draft

## Styling Joins TypeScript And JavaScript Analysis

Fallow v3 expands the static analysis surface from TypeScript and JavaScript into styling. `fallow audit` now includes styling feedback by default, so AI-generated changes can be reviewed for design-system drift before they land.

The styling pass is static, advisory, and verify-first. It does not edit files automatically. It surfaces reviewable evidence for new raw colors, type sizes, raw style values, duplicate declaration blocks, broken styling references, dead styling surface, and design-token blast radius.

### Supported Styling Surfaces

| Surface | v3 coverage |
| --- | --- |
| CSS | First-class parsing for selector complexity, duplicate declaration blocks, token drift, dead surface, broken references, unused keyframes, unused custom properties, unused font faces, and token consumers. |
| Sass / Less | Conservative parser-level scanning for Sass, SCSS, and Less. No Sass/Less compiler expansion. |
| Tailwind / shadcn / CVA | Tailwind arbitrary values, Tailwind v4 `@theme` token consumers, unused theme tokens, CVA duplicate variant blocks, and CVA variant token drift. |
| StyleX | `defineVars` token definitions, member-access consumers, nearest-token suggestions, and token blast-radius. |
| PandaCSS | `defineTokens` token definitions, `token(...)` consumers, nearest-token suggestions, and token blast-radius. |
| vanilla-extract | `createTheme`, `createThemeContract`, and `createGlobalTheme` token definitions, member-access consumers, nearest-token suggestions, and token blast-radius. |
| styled-components | Static theme object tokens when a theme provider is present, plus theme member-access consumers and raw style evidence. |
| Emotion | Static theme object tokens when Emotion theme provider usage is present, plus theme member-access consumers and raw style evidence. |
| CSS Modules | Static CSS module imports and exported class names contribute to styling reachability checks. |

### Suggested Announcement Copy

Fallow now covers styling alongside TypeScript and JavaScript: CSS, Sass/Less parser-level scans, CSS Modules, Tailwind/shadcn/CVA, StyleX, PandaCSS, vanilla-extract, styled-components, and Emotion. It helps keep AI-generated work aligned with the project design system by surfacing new colors, type sizes, raw style values, duplicate style blocks, broken styling references, and dead styling surface as reviewable audit feedback.

Styling findings are intentionally advisory. Fallow points to what changed and what looks inconsistent; humans and agents still verify the context before editing or deleting styling.
