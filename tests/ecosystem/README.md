# Ecosystem Tests

Tests fallow against real-world open-source TypeScript/JavaScript projects to catch crashes, panics, and regressions that unit tests and fixtures may miss.

## What this tests

The script clones popular JS/TS projects (shallow, depth=1), optionally installs their dependencies, and runs `fallow dead-code --format json --quiet` against each one. It distinguishes between:

- **Exit 0**: fallow ran successfully, no issues found (rare for large projects)
- **Exit 1**: fallow ran successfully, issues found (expected and normal)
- **Exit 2+**: fallow crashed (panic, parse error, OOM, etc.): this is a test failure
- **Timeout**: fallow took longer than 5 minutes on a single project: treated as a crash

The test passes if no project causes a crash. Finding dead code issues (exit 1) is expected behavior, not a failure.

## Projects tested

| Project | Repo | Why |
|---------|------|-----|
| next.js | `vercel/next.js` | Very large monorepo, Next.js framework |
| vite | `vitejs/vite` | Monorepo with pnpm workspaces |
| vue-core | `vuejs/core` | TypeScript-heavy, Vue 3 framework |
| svelte | `sveltejs/svelte` | SFC parsing, Svelte framework |
| remix | `remix-run/remix` | React Router, Remix framework |
| trpc | `trpc/trpc` | TypeScript monorepo |
| create-t3-app | `t3-oss/create-t3-app` | Full-stack template |
| query | `TanStack/query` | Popular library |
| jest | `jestjs/jest` | Testing framework, yarn workspaces |
| storybook | `storybookjs/storybook` | UI dev tool, large monorepo |
| tailwindcss | `tailwindlabs/tailwindcss` | CSS framework |
| prisma | `prisma/prisma` | Database ORM |

## Running locally

```bash
# Build and run (builds fallow in release mode first)
./tests/ecosystem/run.sh

# Use an existing binary
./tests/ecosystem/run.sh --fallow-bin ./target/release/fallow

# Or via environment variable
FALLOW_BIN=./target/release/fallow ./tests/ecosystem/run.sh

# Custom clone directory (default: /tmp/fallow-ecosystem)
ECOSYSTEM_DIR=~/ecosystem-tests ./tests/ecosystem/run.sh
```

Cloned repos are cached in `ECOSYSTEM_DIR` and reused across runs. Delete the directory to start fresh.

## Adding a new project

Edit the `PROJECTS` array in `run.sh`. Each entry has four fields:

```
"org/repo  branch  subdirectory  install_command"
```

- **org/repo**: GitHub repository (e.g., `vercel/next.js`)
- **branch**: Branch to clone (e.g., `main`, `canary`)
- **subdirectory**: Subdirectory to analyze, use `.` for the repo root
- **install_command**: Command to install deps, use `-` to skip installation

Example:

```bash
"facebook/react  main  .  yarn install --frozen-lockfile --ignore-scripts"
```

Use `--ignore-scripts` (or equivalent) to skip postinstall scripts that may compile native modules or run builds.

## CI

The `ecosystem-full.yml` workflow runs:

- **Weekly** on Sundays at 04:00 UTC (cron schedule)
- **On demand** via manual workflow_dispatch trigger

It builds fallow in release mode, clones all projects, runs the test script, and uploads JSON results plus per-project stderr logs as artifacts. The workflow fails if any project causes a crash.

There is also a lighter `ecosystem.yml` workflow that runs on push/PR with a smaller set of projects.
