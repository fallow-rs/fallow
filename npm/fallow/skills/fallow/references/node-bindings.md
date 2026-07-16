# Node.js Bindings

When embedding fallow inside a Node.js process (editor extensions, long-running servers, custom tooling), prefer the NAPI bindings over spawning the CLI. Same analysis engine, same JSON envelopes, no subprocess or JSON parsing overhead.

```bash
npm install @fallow-cli/fallow-node
```

```ts
import { detectDeadCode, detectDuplication, computeHealth } from '@fallow-cli/fallow-node';

const deadCode = await detectDeadCode({ root: process.cwd(), explain: true });
const dupes = await detectDuplication({ root: process.cwd(), mode: 'mild', minTokens: 30 });
const health = await computeHealth({ root: process.cwd(), score: true, ownershipEmails: 'handle' });
```

Seven async functions: `detectDeadCode`, `detectCircularDependencies`, `detectBoundaryViolations`, `detectDuplication`, `detectFeatureFlags`, `computeComplexity`, `computeHealth`. Each returns the same JSON envelope the CLI emits for `--format json`. Rejected promises throw a `FallowNodeError` with `message`, `exitCode`, and optional `code`, `help`, `context` fields that mirror the CLI's structured error surface.

Enum-like fields take lowercase CLI-style literals (`"mild"`, `"cyclomatic"`, `"handle"`, `"low"`). Write-path commands (`fix`, `init`, `hooks install`, `hooks uninstall`, `license activate`, `coverage setup`) are not exposed; use the CLI for those.

See <https://docs.fallow.tools/integrations/node-bindings> for the full field reference.
