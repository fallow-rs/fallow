### Fixed

- **`fallow security` reports fewer source-free candidates in library, build and CLI code.** On eight public projects (zod, hono, trpc, vite, astro, excalidraw, next.js, vscode), most `path-traversal`, `dynamic-regex` and `header-injection` candidates had no untrusted input near them. These changes keep the request-driven candidates and drop the rest:
  - `RegExp(x)`, `new RegExp(x)`, `res.setHeader(name, x)` and `res.writeHead(status, x)` now need a value that traces to untrusted input, as `redos-regex` already did.
  - `path.join` and `path.resolve` now need untrusted input in the argument, or in the same module. A CLI argument does not count, because the user who runs a CLI owns its file system. The `node:fs` path methods do not change.
  - `req.url`, `req.originalUrl` and `req.headers` (also on `request`) are now untrusted request input, so a static-file middleware that joins a path from `req.url` stays a candidate. Code that logs or copies these values can show a new `secret-pii-log` or `mass-assignment` candidate.
  - A test, story or tooling-config file no longer starts an untrusted-source trace. Before, a request read in `app.test.ts` raised the severity of the code under test.
  - `process.env` and `import.meta.env` are deployment secrets, not attacker input. They still back `secret-to-network` and `secret-pii-log`. They no longer make another sink source-backed, and they no longer start an untrusted-source trace.
  - The message-event source now needs a message-event name before `.data` (`event`, `e`, `evt`, `ev`, `msg`, `message`, `messageEvent`). Before, any `.data` read counted, for example `input.data` of a parse result. The `.search` and `.hash` sources now need `location` before them, so a content hash such as `crypto.hash` does not count.
  - The identifiers in the computed keys and spreads of an object argument now count for a source trace, so `res.writeHead(302, { ...headers })` with request headers is source-backed. This changes the cached extraction data, so the first run after the upgrade parses every file again.

### Changed

- **The secret-leak `client-server-leak` finding now has `category: "client-server-leak"`.** Every security finding now names a category, so a consumer can group candidates on `category` alone. The rule id, the `finding_id` and the SARIF fingerprint do not change.
