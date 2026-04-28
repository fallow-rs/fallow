# Fallow: Critical Gotchas

Common pitfalls and their correct solutions when working with fallow.

---

## `fix` Requires `--yes` in Non-TTY Environments

The `fix` command prompts for confirmation in interactive terminals. In agent subprocesses, CI pipelines, or piped input (non-TTY), the `--yes` flag is mandatory. Without it, `fix` exits with code 2 and an error.

```bash
# WRONG: fix exits with code 2 in non-TTY
fallow fix --format json --quiet

# CORRECT: always use --dry-run first, then --yes
fallow fix --dry-run --format json --quiet   # preview
fallow fix --yes --format json --quiet       # apply
```

Always preview with `--dry-run` before applying. This is a destructive operation that modifies source files.

---

## Don't Create Config Unless Needed

Fallow works with zero configuration for most projects thanks to 90 auto-detecting framework plugins. Creating an unnecessary config file can mask issues or override detection behavior.

```bash
# WRONG: creating config for a standard Next.js project
fallow init
# This may override auto-detected settings

# CORRECT: run analysis first with zero config
fallow dead-code --format json --quiet
# Only create config if you need to customize rules, ignore patterns, or entry points
```

Only create a config when you need to:
- Change rule severity levels for incremental adoption
- Add custom ignore patterns or ignore dependencies
- Specify additional entry points not auto-detected
- Configure duplication detection settings

---

## Use `--format json` for Agent Consumption

Human-formatted output contains ANSI colors, progress bars, and timing info. Never parse it programmatically.

```bash
# WRONG: parsing human output
fallow dead-code | grep "unused"

# CORRECT: use structured JSON
fallow dead-code --format json --quiet
```

The `--quiet` flag suppresses progress bars on stderr. Without it, stderr output may interfere with stdout parsing.

---

## `--changed-since` Shows Only New Issues

The `--changed-since` flag limits analysis to files modified since a git ref. It only reports issues in those files, not all issues in the project. Works with both `dead-code` and `dupes`.

```bash
# This only shows issues in files changed since main
fallow dead-code --format json --quiet --changed-since main

# Same for duplication — only clone groups involving changed files
fallow dupes --format json --quiet --changed-since main

# This shows ALL issues in the project
fallow dead-code --format json --quiet
```

Don't use `--changed-since` when auditing the full project. Use it for PR checks and incremental CI.

---

## Filter Flags Are Additive

Issue type filter flags (`--unused-exports`, `--unused-files`, etc.) are inclusive. They select which issue types to show. Using multiple flags shows the union.

```bash
# Shows only unused exports
fallow dead-code --format json --quiet --unused-exports

# Shows unused exports AND unused files
fallow dead-code --format json --quiet --unused-exports --unused-files

# Shows ALL issue types (default when no filter is specified)
fallow dead-code --format json --quiet
```

---

## Syntactic Analysis: No TypeScript Compiler

Fallow uses Oxc for pure syntactic analysis. It does not run the TypeScript compiler. This means:

- **Fully dynamic imports** (`import(variable)`) are not resolved. Only static strings, template literals with static prefixes, `import.meta.glob`, and `require.context` patterns
- **Value-level type narrowing** is not performed. Fallow can't know that `if (x instanceof Foo)` means `Foo` is "used"
- **Conditional exports** based on runtime values are not analyzed
- **Function overload signatures are deduplicated**: TypeScript function overloads (multiple signatures for the same function name) are merged into a single export. They are not reported as separate unused exports

```typescript
// RESOLVED: static pattern with prefix
import(`./locales/${lang}.json`);

// RESOLVED: import.meta.glob
const modules = import.meta.glob('./modules/*.ts');

// NOT RESOLVED: fully dynamic
const mod = import(someVariable);
```

If fallow falsely flags something due to dynamic patterns, use inline suppression:

```typescript
// fallow-ignore-next-line unused-export
export const dynamicallyUsed = createHandler();
```

---

## Re-Export Chains Are Resolved

Fallow fully resolves `export *` and named re-export chains through barrel files. An export consumed through a chain of barrel files is NOT falsely flagged.

```typescript
// src/utils.ts
export const helper = () => {};  // NOT flagged, used via barrel chain

// src/index.ts (barrel)
export * from './utils';

// src/app.ts
import { helper } from './index';  // Resolves through the chain
```

If an export IS flagged as unused despite being in a barrel file, it means no downstream consumer actually imports it. The barrel file re-exports it, but nobody uses it from there.

---

## Exit Code 1 vs 2

| Code | Meaning | Action |
|------|---------|--------|
| 0 | No error-severity issues | Success |
| 1 | Error-severity issues found | Review findings |
| 2 | Runtime error (`fix` without `--yes` in non-TTY, invalid config) | Fix config or add `--yes` |

Exit code 1 is triggered by issues with `"error"` severity in the rules config. Without a rules section, all issue types default to `"error"`. Use the rules system to control which issues fail CI:

```jsonc
// Only fail on unused files and deps, warn on everything else
{
  "rules": {
    "unused-files": "error",
    "unused-dependencies": "error",
    "unused-exports": "warn",
    "unused-types": "warn"
  }
}
```

---

## `--fail-on-issues` Promotes Warn to Error

The `--fail-on-issues` flag promotes all `warn`-severity rules to `error` for that run. This means exit code 1 for ANY reported issue.

```bash
# With rules: { "unused-exports": "warn" }

# This exits 0 even with warn-level findings
fallow dead-code --format json --quiet

# This exits 1 if ANY issue is found (warn promoted to error)
fallow dead-code --format json --quiet --fail-on-issues
```

Use `--fail-on-issues` for strict CI gates. Use the rules system for gradual adoption.

---

## Baseline Comparison Tracks Issue Identity

Baselines track issues by identity (file + issue type + name), not by count. Adding a new unused export while fixing an old one doesn't cancel out.

```bash
# Save current state as baseline
fallow dead-code --format json --quiet --save-baseline fallow-baselines/dead-code.json

# Later: only fail on NEW issues not in the baseline
fallow dead-code --format json --quiet --baseline fallow-baselines/dead-code.json --fail-on-issues
```

Commit the baseline file to your repo. Update it periodically as you fix existing issues.

---

## Duplication Modes Affect What's Detected

The detection mode significantly affects results. Choose based on your needs:

```bash
# strict: exact token match only
fallow dupes --format json --quiet --mode strict
# Catches: copy-pasted code with zero changes

# mild (default): syntax normalized
fallow dupes --format json --quiet --mode mild
# Catches: whitespace and semicolon differences

# weak: literal values normalized
fallow dupes --format json --quiet --mode weak
# Catches: same structure with different strings/numbers

# semantic: identifier names normalized
fallow dupes --format json --quiet --mode semantic
# Catches: same logic with renamed variables
```

`semantic` mode produces the most findings but may include false positives where similar structure is coincidental.

---

## Workspace Flag Scopes Output, Not Analysis

The `--workspace` flag scopes **output** to a single package, but the full cross-workspace module graph is still built. This means:

- Imports from other workspace packages are still resolved
- Re-export chains crossing package boundaries are still tracked
- Only issues IN the specified package are reported

```bash
# Analyze everything, show only issues in "my-package"
fallow dead-code --format json --quiet --workspace my-package
```

---

## Production Mode Excludes Test Files

`--production` excludes test/dev files and only analyzes production scripts. This changes what's reported:

- Test files (`*.test.*`, `*.spec.*`, `*.stories.*`, `__tests__/**`) are excluded
- Only `start`, `build`, `serve`, `preview`, `prepare` scripts are analyzed
- Unused devDependencies are NOT reported (forced to `off`)
- Type-only production dependencies ARE reported (should be devDependencies)

```bash
# WRONG: using --production for a full audit
fallow dead-code --format json --quiet --production
# Misses test-file dead code and devDependency issues

# CORRECT: use --production only for production-focused CI
fallow dead-code --format json --quiet --production --fail-on-issues
```

---

## Watch Mode Is Not for Agents

The `watch` command starts an interactive file watcher that never exits. Never use it in agent workflows.

```bash
# WRONG: this will hang forever
fallow watch

# CORRECT: run one-shot analysis
fallow dead-code --format json --quiet
```

---

## Suppressing Duplication False Positives

Code duplication has its own suppression token: `code-duplication`. Use it for intentionally similar code (e.g., test helpers, generated patterns).

```typescript
// WRONG: using the wrong token
// fallow-ignore-file unused-export
// This suppresses dead code, not duplication

// CORRECT: suppress duplication for a specific line
// fallow-ignore-next-line code-duplication
const handler = createStandardHandler(config);

// CORRECT: suppress all duplication in a file
// fallow-ignore-file code-duplication
```

This is separate from the dead code suppression tokens. See the full list of valid tokens in the [CLI Reference](cli-reference.md#inline-suppression-comments).

---

## Decorated Members Are Skipped

Class members with decorators (NestJS `@Get()`, Angular `@Input()`, TypeORM `@Column()`, etc.) are automatically excluded from unused member detection. Decorator-driven frameworks consume these via reflection at runtime.

```typescript
class UserController {
  @Get('/users')
  getUsers() { ... }  // NOT flagged, has decorator
}
```

This is handled automatically. No suppression needed.

---

## JSDoc Visibility Tags Keep Exports Alive

Exports annotated with `/** @public */`, `/** @internal */`, `/** @beta */`, `/** @alpha */`, or `/** @api public */` are never reported as unused. This is designed for library authors whose exports are consumed by external projects not visible to fallow.

```typescript
// NOT flagged: @public annotation
/** @public */
export const createWidget = () => {};

// NOT flagged: @internal annotation
/** @internal */
export const resetState = () => {};

// NOT flagged: @beta annotation
/** @beta */
export const experimentalFeature = () => {};

// NOT flagged: @alpha annotation
/** @alpha */
export const unstableApi = () => {};

// NOT flagged: @api public variant
/** @api public */
export interface WidgetConfig {}

// STILL flagged: line comments don't count
// @public
export const notProtected = () => {};
```

Only `/** */` JSDoc block comments are recognized. Line comments (`// @public`) are ignored.

---

## `@expected-unused` JSDoc Tag for Intentional Dead Code

Exports annotated with `/** @expected-unused */` are treated as intentionally unused. They are excluded from unused export detection AND tracked for staleness. If the export later becomes used (imported by another module), fallow reports the `@expected-unused` tag as stale via the `stale-suppressions` rule.

```typescript
// NOT flagged as unused: @expected-unused annotation
/** @expected-unused */
export const deprecatedHelper = () => {};

// If something starts importing deprecatedHelper,
// fallow reports the @expected-unused tag as stale
```

Use `@expected-unused` instead of `// fallow-ignore-next-line` when you want fallow to notify you if the export becomes referenced again. The `stale-suppressions` rule (default: `warn`) controls severity.

Only `/** */` JSDoc block comments are recognized. The tag works on all export types.

---

## Stale Suppression Detection

Fallow detects `// fallow-ignore` comments and `@expected-unused` JSDoc tags that no longer match any issue. This prevents suppression comments from silently hiding issues that have been resolved or moved.

```typescript
// STALE: the export below is actually used now
// fallow-ignore-next-line unused-export
export const helper = () => {};  // imported in app.ts
```

Use `--stale-suppressions` to filter for only stale suppression findings. The `stale-suppressions` rule defaults to `warn`. Set to `error` in CI to enforce suppression hygiene:

```jsonc
{
  "rules": {
    "stale-suppressions": "error"
  }
}
```

---

## JSDoc `import()` Types Count as References

Types referenced only from JSDoc `import()` annotations are tracked as type-only imports, so the referenced exports are not flagged as unused. This works for plain JavaScript files that want TypeScript types without converting to `.ts`.

```js
// src/app.js

/**
 * @param cfg {import('./types.ts').Config}
 * @returns {import('./types.ts').Result}
 */
function boot(cfg) {
  return { ok: true };
}
```

Fallow treats `Config` and `Result` in `./types.ts` as used. Works with `@param`, `@returns`, `@type`, `@typedef`, `@callback`, union annotations (`{import('./a').A | import('./b').B}`), nested member access, bare package specifiers, and parent-relative paths. Only `/** */` blocks are scanned.

---

## JSX `<script src>` and `<link href>` Are Asset References

Inside JSX/TSX files, lowercase intrinsic `<script src="...">` and `<link rel="stylesheet|modulepreload" href="...">` are tracked as asset references, same as in plain HTML files. This is needed for SSR frameworks like Hono where layout components emit HTML via JSX.

```tsx
// src/layout.tsx

export const Layout = () => (
  <html>
    <head>
      <link rel="stylesheet" href="/static/style.css" />
      <script src="/static/app.js"></script>
    </head>
    <body><h1>Hello</h1></body>
  </html>
);
```

Fallow marks `static/style.css` and `static/app.js` as reachable. Root-relative paths (starting with `/`) resolve from the source file's parent directory first, then the project root, matching how Vite/Parcel/Hono serve static assets. Only `StringLiteral` attribute values are captured: expression containers (`href={someVar}`) and capitalized React-style components (`<Script>`, `<Link>`) are intentionally ignored because they have component-specific semantics.

---

## Library Packages: Use `publicPackages` Instead of Visibility Tags

In monorepos, shared library packages have exports consumed by external consumers not visible to fallow. Instead of annotating every export with `/** @public */` (or `@internal`, `@beta`, `@alpha`), use the `publicPackages` config to mark entire workspace packages as public libraries. All exports from these packages are excluded from unused export detection.

```jsonc
{
  "publicPackages": ["@myorg/shared-lib", "@myorg/ui-kit"]
}
```

This is the correct solution for library false positives in monorepos. Only use JSDoc visibility tags (`/** @public */`, `/** @internal */`, etc.) for individual exports in application packages.

---

## Dynamically Loaded Files: Use `dynamicallyLoaded`

Files loaded at runtime via plugin systems, locale directories, or lazy module patterns are not statically reachable from entry points. Use `dynamicallyLoaded` to mark these files as always-used.

```jsonc
{
  "dynamicallyLoaded": ["plugins/**/*.ts", "locales/**/*.json"]
}
```

```bash
# WRONG: suppressing individual files
# fallow-ignore-file unused-file  (in each plugin file)

# CORRECT: declare the pattern in config
# { "dynamicallyLoaded": ["plugins/**/*.ts"] }
```

This is preferable to adding inline suppression comments to every dynamically loaded file.

---

## Class Instance Members Are Tracked

Fallow tracks class member usage through instance variables. If you instantiate a class and call methods on the instance, those members are correctly marked as used.

```typescript
class MyService {
  greet() { return 'hello'; }   // NOT flagged: used via instance
  unused() { return 'bye'; }    // Flagged: never called
}

const svc = new MyService();
svc.greet();
```

This also handles whole-object instance patterns (`Object.values(svc)`, `{ ...svc }`, `for..in`) conservatively (all members marked as used). The tracking is scope-unaware, so same-named variables in different scopes may produce false negatives (not false positives).

---

## Type-Only Dependencies Should Be devDependencies

In `--production` mode, fallow detects production dependencies that are only imported via `import type`. Since TypeScript types are erased at runtime, these packages should be in `devDependencies` instead.

```typescript
// If "zod" is in dependencies (not devDependencies):
import type { ZodSchema } from 'zod';  // Flagged as type-only dependency

// This is a real import, not type-only:
import { z } from 'zod';  // NOT flagged
```

```bash
# Detect type-only dependencies (reported automatically with --production)
fallow dead-code --format json --quiet --production

# Suppress for a specific dependency
# fallow-ignore-next-line type-only-dependency
```

The `type-only-dependencies` rule defaults to `warn`. Suppress with `"type-only-dependencies": "off"` in your rules config if you intentionally keep type-only packages in production dependencies.

---

## Test-Only Dependencies Should Be devDependencies

Fallow detects production dependencies that are only imported from test files (`*.test.*`, `*.spec.*`, `__tests__/**`). Since these packages are never used in production code, they should be in `devDependencies` instead.

```typescript
// If "msw" is in dependencies (not devDependencies):
// src/handlers.test.ts
import { setupServer } from 'msw/node';  // Flagged as test-only dependency

// src/app.ts — no imports of "msw" here
```

```bash
# Detect test-only dependencies (reported automatically)
fallow dead-code --format json --quiet

# Suppress for a specific dependency
# fallow-ignore-next-line test-only-dependency
```

The `test-only-dependencies` rule defaults to `warn`. Suppress with `"test-only-dependencies": "off"` in your rules config if you intentionally keep test-only packages in production dependencies.

---

## GitLab CI: `FALLOW_COMMENT` vs `FALLOW_REVIEW`

These are separate features and can be used independently or together:

- **`FALLOW_COMMENT: "true"`** — posts a single summary comment on the MR with issue counts and a findings table
- **`FALLOW_REVIEW: "true"`** — posts inline code review comments on the exact MR diff lines where issues were found

```yaml
# WRONG: expecting inline review comments from FALLOW_COMMENT
variables:
  FALLOW_COMMENT: "true"
# This only posts a summary comment, not inline annotations

# CORRECT: use FALLOW_REVIEW for inline diff comments
variables:
  FALLOW_REVIEW: "true"

# CORRECT: use both for summary + inline
variables:
  FALLOW_COMMENT: "true"
  FALLOW_REVIEW: "true"
```

Both require a `GITLAB_TOKEN` CI/CD variable (project access token with `api` scope). `CI_JOB_TOKEN` is read-only for MR notes in the official GitLab API, so it is not enough for summary comments or inline discussions.

---

## License Errors Include a Machine-Readable Code Suffix

`fallow license refresh` and `fallow license activate --trial` can fail with a backend error. The CLI always appends the raw HTTP status and the backend error code after the human hint, so scripts can grep for the code without parsing prose:

```
fallow license refresh: your stored license is too stale to refresh. Reactivate with: fallow license activate --trial --email <addr> (HTTP 401, code token_stale)
```

Stable codes the CLI surfaces today:

| Code | Operation | Meaning |
|------|-----------|---------|
| `token_stale` | `refresh` | Stored JWT is more than 45 days past its `exp`. Reactivate. |
| `invalid_token` | `refresh` | Stored JWT is missing required claims (e.g. `sub`). Reactivate. |
| `unauthorized` | `refresh` or `trial` | Auth failed. Reactivate. |
| `rate_limit_exceeded` | `trial` | Trial endpoint is capped at 5 per hour per IP. Wait or use a different network. |

To detect a rate-limited trial signup in CI:

```bash
if fallow license activate --trial --email "$EMAIL" 2>&1 | grep -q "code rate_limit_exceeded"; then
  echo "Trial rate-limited; fallback to cached FALLOW_LICENSE" >&2
fi
```

Unknown codes fall back to the backend `message` field when present, otherwise the raw body, so existing scripts that match on HTTP status alone still work.

---

## GitLab CI: Auto `--changed-since` in MR Pipelines

The official GitLab CI template automatically sets `--changed-since origin/$CI_MERGE_REQUEST_TARGET_BRANCH_NAME` in merge request pipelines. You do not need to set `FALLOW_CHANGED_SINCE` manually unless you want a different ref.

```yaml
# UNNECESSARY: changed-since is auto-detected in MR pipelines
variables:
  FALLOW_CHANGED_SINCE: "origin/main"

# CORRECT: let the template auto-detect
# (no FALLOW_CHANGED_SINCE needed — it reads the MR target branch)
```

Override `FALLOW_CHANGED_SINCE` only when you need a specific ref (e.g., a release branch) or want to disable auto-detection by setting it to an empty string.

---

## GitLab CI: Package Manager Detection

The GitLab CI template auto-detects the project's package manager from lockfiles (`package-lock.json` for npm, `pnpm-lock.yaml` for pnpm, `yarn.lock` for yarn). MR comments and review comments use the correct commands for the detected manager.

This means review comments will show `pnpm remove lodash` instead of `npm uninstall lodash` in a pnpm project. No configuration is needed — detection is automatic.
