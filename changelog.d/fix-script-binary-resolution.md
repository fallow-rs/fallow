### Fixed

- **Workspace package binaries now credit their package without
  `node_modules`.** Fallow reads the `bin` field of each workspace package
  that a manifest declares. A script that calls such a binary marks the
  declaring package as used.
- **More binaries map to their package.** `napi` and `napi-raw` now credit
  `@napi-rs/cli`, `taplo` credits `@taplo/cli`, `oxnode` credits
  `@oxc-node/cli`, and `ut` and `utx` credit `utoo`.
- **GitHub Actions steps resolve file arguments in their
  `working-directory`.** Fallow reads the step value and the
  `defaults.run.working-directory` value of the workflow and of the job.
  Paths that start with `$GITHUB_WORKSPACE/` or `${{ github.workspace }}/`
  resolve from the project root, and so do GitLab `$CI_PROJECT_DIR/` paths.
  A relative path in a directory outside the project, or in a directory with
  a run-time value, makes no entry point.
- **Fallow now reads composite actions.** It reads the run steps in
  `.github/actions/*/action.yml`, `.github/actions/*/*/action.yml` and the
  root `action.yml` like workflow steps. `${{ github.action_path }}/` paths
  resolve from the action directory.
