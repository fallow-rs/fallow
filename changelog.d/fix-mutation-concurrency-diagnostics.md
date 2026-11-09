### Fixed

- Mutation errors from restarted companions suggest `--concurrency 1` when the phase uses parallel workers. The hint covers init and dry-run failures and missing covering tests, and preserves the original error and timeout guidance. Serial phases keep their existing diagnostics. Fixes #4363.
