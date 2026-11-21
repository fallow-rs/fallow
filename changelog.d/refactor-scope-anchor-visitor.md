### Fixed

- **A diff scope no longer keeps old boundary, policy and component findings.**
  `--diff-file`, `--diff-stdin` and the CI diff now narrow boundary call
  violations, rule-pack policy violations, enforced thin wrappers and
  enforced duplicate prop shapes to added lines, and boundary coverage
  violations to touched files.
- **A pure rename that creates a route conflict is reported in a diff scope.**
  Route collisions and dynamic segment name conflicts use the touched file,
  not line 1.
- **`--workspace` narrows prop-drilling chains, thin wrappers and duplicate
  prop shapes to the selected workspaces.**
