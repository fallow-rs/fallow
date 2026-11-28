### Fixed

- **Coverage paths with `..` match the same source file on each machine.** fallow applied a `..` in a recorded coverage path only when the directory before it existed, so a file could get its coverage on one machine and a static estimate on another. fallow now applies each `..` to the recorded text first.
