# Node.js Bindings

When embedding fallow inside a Node.js process (editor extensions, long-running servers, custom tooling), prefer the NAPI bindings over spawning the CLI. Same analysis engine, same JSON envelopes, no subprocess or JSON parsing overhead.

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

Eight async functions: `detectDeadCode`, `detectCircularDependencies`, `detectBoundaryViolations`, `detectDuplication`, `detectSimilarCode`, `detectFeatureFlags`, `computeComplexity`, `computeHealth`. Each returns the same JSON envelope the CLI emits for `--format json`. `detectSimilarCode` returns a precisely typed `SimilarCodeReport`, including generation provenance, embedding semantics, effective `generation.scope.paths`, completion, skips, cache accounting, diagnostics, and read-only candidate actions. Treat the materialized scope as provenance and preserve the raw report when a candidate may be inspected later. The Node binding exposes discovery only. Use CLI `similar-code inspect --candidates <report.json>` or MCP `inspect_similar_code` with the exact typed candidate snapshot so global retrieval and ranking are not repeated. Its loader resolves and verifies the exact-version local companion; it never downloads the model or authorizes setup. Rejected promises throw a `FallowNodeError` with `message`, `exitCode`, and optional `code`, `help`, `context` fields that mirror the CLI's structured error surface.

Enum-like fields take lowercase CLI-style literals (`"mild"`, `"cyclomatic"`, `"handle"`, `"low"`). Write-path commands (`fix`, `init`, `hooks install`, `hooks uninstall`, `license activate`, `coverage setup`) are not exposed; use the CLI for those.

See <https://docs.fallow.tools/integrations/node-bindings> for the full field reference.
