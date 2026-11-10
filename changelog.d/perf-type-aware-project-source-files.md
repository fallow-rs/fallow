### Performance

- **`--type-aware` transfers fewer source files from the TypeScript checker.** The semantic pass read every program file, including each declaration file below `node_modules`, to find the project source files, and it did so several times in each request. It now skips files below `node_modules` and declaration files by their name, and it reads the list one time for each program. On the Next.js repository with its root `tsconfig.json`, a request transfers 4,006 files instead of 10,130. The results do not change.
