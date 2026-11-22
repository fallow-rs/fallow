### Fixed

- **A `package.json` that starts with a byte order mark is read by every dependency check.** Before, the unused-dependency check of a workspace, the unused-override check and the catalog checks did not read such a manifest. So `fallow dead-code` did not report an unused dependency of that workspace, and it lost the `catalog:` references of that manifest. Now these checks ignore a leading UTF-8 byte order mark, as npm and Node.js do.
