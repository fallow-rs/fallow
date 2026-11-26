### Fixed

- **Windows: a folder of another user above the project no longer widens the read scope.** A `.git` entry or a workspace manifest in a folder above the analysis root, such as `C:\Users\Public` or the root of a shared drive, gave the shared configs of that folder to every project below it. Now such a folder counts only when the current user or the owner of the analysis root owns it. This is the owner rule that Fallow already uses on Linux and macOS.
