# Styling Release Matrix

Short v3 release-note matrix for the styling integration.

| Surface | Release note claim | Confidence and caveats |
| --- | --- | --- |
| CSS | Parses standard CSS for selector complexity, duplicate declaration blocks, token drift, dead styling surface, broken references, unused keyframes, unused custom properties, unused font faces, and token consumers. | Strong first-class support. Findings are report-only with verification actions. |
| Sass / Less | Scans Sass, SCSS, and Less at parser/text level where fallow can stay conservative. | Parser-level only. No Sass/Less compiler expansion, so project-wide class reachability abstains when preprocessor stylesheets dominate. |
| Tailwind / shadcn / CVA | Detects Tailwind arbitrary values, Tailwind v4 `@theme` token consumers, unused theme tokens, CVA duplicate variant blocks, and CVA variant token drift. | Strong for static class strings and `@theme`. CVA token drift is verify-first because variants can encode product semantics. |
| StyleX | Detects `defineVars` token definitions, member-access consumers, raw-style nearest-token suggestions, and token blast-radius. | Supports relative imports, tsconfig `paths` aliases, and workspace package imports through the shared resolver. |
| PandaCSS | Detects `defineTokens` and static `defineConfig` token definitions, `token(...)` and static style-object token consumers, raw-style nearest-token suggestions, and token blast-radius. | Supports generated `styled-system` token imports, including path-aliased specifiers that still contain the `styled-system` segment. Static `theme.tokens` and `theme.semanticTokens` are read without executing Panda config. |
| vanilla-extract | Detects `createTheme`, `createThemeContract`, and `createGlobalTheme` token definitions, member-access consumers, raw-style nearest-token suggestions, and token blast-radius. | Supports relative imports, tsconfig `paths` aliases, and workspace package imports through the shared resolver. |
| styled-components | Detects static theme object tokens when a `ThemeProvider` is present, plus theme member-access consumers and raw style evidence. | Conservative by design. Dynamic theme construction remains a lower-bound signal. |
| Emotion | Detects static theme object tokens when Emotion theme provider usage is present, plus theme member-access consumers and raw style evidence. | Conservative by design. Dynamic theme construction remains a lower-bound signal. |
| CSS Modules | Treats CSS module exports and imports as styling surface for dead-surface and reference checks. | Strong for static module imports and exported class names. Dynamic class assembly remains verify-first. |

Suggested release phrasing:

> Fallow now covers styling alongside TypeScript and JavaScript: CSS, Sass/Less
> parser-level scans, CSS Modules, Tailwind/shadcn/CVA, StyleX, PandaCSS,
> vanilla-extract, styled-components, and Emotion. It keeps AI-generated work
> aligned with the project's existing design system by surfacing new colors,
> type sizes, raw style values, duplicate style blocks, broken styling
> references, and dead styling surface as reviewable audit feedback.
