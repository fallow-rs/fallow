### Security

- **Workspace globs no longer read directories outside the project root.**
  An entry such as `../*` or an absolute path in `package.json` `workspaces`,
  `pnpm-workspace.yaml` `packages`, `deno.json` `workspace`, or the
  `workspaces.patterns` config listed directories outside the root. Fallow
  added no package from them, but a directory without a `package.json` got a
  `glob-matched-no-package-json` diagnostic with its outside path. Fallow now
  skips such an entry before it reads the disk. A new
  `workspace-glob-outside-root` diagnostic names the entry and the file that
  declares it. The run does not exit with an error, because npm, pnpm, and
  yarn accept a `../` workspace entry. A `..` that stays inside the root,
  as in `packages/../libs/*`, still works. The findings do not change.
