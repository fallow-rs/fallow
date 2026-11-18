### Fixed

- **On Windows, the React Router `.client` and `.server` markers now apply in a workspace package.** `client-server-leak` read a workspace package with the React Router plugin as out of scope, because its scope kept the backslashes of the Windows path. A `*.client.*` file in such a package was then no client root, and a `*.server.*` file was no `server-only-import` sink. A root project and macOS and Linux were not affected.
