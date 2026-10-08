# Node.js Bindings

To embed fallow in a Node.js process, use the NAPI bindings. They use the same analysis engine and JSON envelopes as the CLI without spawning the CLI or parsing its JSON output. Examples include editor extensions, long-running servers, and custom tooling.

```bash
npm install @fallow-cli/fallow-node
```

```ts
import { computeHealth, detectDeadCode, detectDuplication, detectSimilarCode } from '@fallow-cli/fallow-node';

const deadCode = await detectDeadCode({ root: process.cwd(), explain: true });
const dupes = await detectDuplication({ root: process.cwd(), mode: 'mild', minTokens: 30 });
const similarCode = await detectSimilarCode({ root: process.cwd(), files: ['src/services/api.ts'] });
const health = await computeHealth({ root: process.cwd(), score: true, ownershipEmails: 'handle' });
```

The async functions are `detectDeadCode`, `detectCircularDependencies`, `detectBoundaryViolations`, `detectArchitecture`, `detectDuplication`, `detectSimilarCode`, `detectFeatureFlags`, `computeComplexity`, and `computeHealth`. Each returns the same JSON envelope the CLI emits for `--format json`. `detectArchitecture` returns the `kind: "architecture"` envelope of `fallow architecture`; `detectCircularDependencies` and `detectBoundaryViolations` return the `dead-code` envelope.

`detectSimilarCode` returns a precisely typed `SimilarCodeReport` with generation provenance, embedding semantics, effective `generation.scope.paths`, completion, skips, cache accounting, diagnostics, and read-only candidate actions. Treat the materialized scope as provenance. Preserve the raw report when a candidate may be inspected later.

`detectSimilarCode` exposes discovery only. Use CLI `similar-code inspect --candidates <report.json>` or MCP `inspect_similar_code` with the exact typed candidate snapshot. This avoids repeating global retrieval and ranking. The loader resolves and verifies the exact-version local companion. It never downloads the model or authorizes setup.

Rejected promises throw a `FallowNodeError` with `message`, `exitCode`, and optional `code`, `help`, `context` fields. These fields match the CLI's structured errors.

Enum-like fields take lowercase CLI-style literals (`"mild"`, `"cyclomatic"`, `"handle"`, `"low"`). Write-path commands (`fix`, `init`, `hooks install`, `hooks uninstall`, `license activate`, `coverage setup`) are not exposed; use the CLI for those.

See <https://fallow.tools/docs/integrations/node-bindings/> for the full field reference.
