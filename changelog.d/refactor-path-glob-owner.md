### Fixed

- **A leading `./` on a path glob matches the same files as the pattern without it.** This covers `entry`, `supportEntry`, `dynamicallyLoaded`, `overrides[].files`, `ignoreExports[].file`, boundary zone patterns and the other path glob fields. Before, such a pattern passed validation and then matched no file. Lists that reject `./` still reject it.
- **`boundaries` `autoDiscover` zones match a directory whose name has glob characters.** A Next.js folder such as `app/[id]` gets a zone that matches only `app/[id]`, not `app/i` or `app/d`.
- **A negated workspace pattern keeps `*` inside one folder level.** `!packages/*` excludes `packages/a` and no longer excludes `packages/a/b`. Write `!packages/**` to exclude each level. `!./packages/skip` excludes `packages/skip`.
- **A recursive workspace pattern works in a checkout whose path has `[`, `*` or `?`, and with a wildcard before `**`, such as `pkg-*/**`.** A single `*` in such a pattern stays inside one folder level. The member patterns of a nested pnpm project in a folder such as `tool[1]` match only that folder.
- **A tsconfig `include` or `exclude` entry keeps `*` and `?` inside one folder level, as tsc does.** `src/*.ts` no longer claims `src/nested/a.ts`.
- **The Convex `functions` folder, the Wrangler `main` file and the Mintlify docs folder are literal paths.** A `[` in the name matches only itself.
