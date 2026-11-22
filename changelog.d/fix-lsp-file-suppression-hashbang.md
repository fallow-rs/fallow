### Fixed

- **The editor action that dismisses a security candidate type in a file keeps the hashbang first.** Before, the action put the `// fallow-ignore-file` comment above a `#!/usr/bin/env node` line. Node.js then failed to start the script. Now the comment goes on the line below the hashbang.
