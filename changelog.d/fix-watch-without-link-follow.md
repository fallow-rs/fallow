### Fixed

- **`fallow watch` no longer follows directory links when it sets up the watch.** Source discovery does not enter a linked directory, and the watcher now does the same. Before, on Linux, a committed link could put a tree outside the project under watch, report project files under a second path, or make the start of the watch walk without end. The target of a link inside the project stays watched under its real path. macOS and Windows did not follow these links before.
