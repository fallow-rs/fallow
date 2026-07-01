# VS Code Agent Guide

Use this file when editing `editors/vscode/**`.

## Ownership

- `src/extension.ts`: activation, commands, lifecycle, and view wiring.
- `src/client.ts`: LSP client setup and middleware.
- `src/commands.ts`: CLI subprocess calls and command arguments.
- `src/download.ts`: managed binary download, verification, and resolution.
- `src/generated/output-contract.d.ts`: generated from `docs/output-schema.json`, never hand-edit.
- `dist/`: gitignored build output, never committed. CI and the release pipeline (`vscode-prep`: `pnpm build` then `pnpm package`) rebuild it fresh from `src/`, so the shipped VSIX always reflects current source.

## Rules

- Source changes only need to compile (`pnpm run build`); the bundle is gitignored and rebuilt by CI/release, never committed.
- Keep generated output types in sync with the Rust schema. Run codegen rather than editing generated files.
- Binary resolution order is user path, workspace dependency, system path, managed binary, then auto-download.
- Do not silently change the VS Code engine floor. It affects Cursor, Windsurf, and VS Code compatibility.
- Health, audit, and sidebar analyses are intentionally separate spawns. Do not fold them together unless the UX and latency tradeoff is explicit.

## Validation

- Source edit: run `pnpm --dir editors/vscode run lint` and the focused tests.
- Generated contract edit: run `pnpm --dir editors/vscode run check:contracts`.
- Bundle-affecting edit: run `pnpm --dir editors/vscode run build` as a compile check. `dist/` is gitignored and rebuilt by CI/release, so there is nothing to commit.
- Packaging or manifest edit: run the manifest or packaging tests that cover the touched surface.
