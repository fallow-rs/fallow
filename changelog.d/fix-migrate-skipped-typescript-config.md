### Fixed

- **`fallow migrate` tells you when it skips a TypeScript source config.** Fallow cannot parse a `.ts` config file. Before, a project with only such a file got the message that no configuration was found, so the found file did not show. Now migrate prints that no configuration could be migrated, names the TypeScript file, and tells you to convert it to JSON first. The exit code stays 2.
