### Fixed

- **A recursive workspace pattern no longer follows a link out of the project or into a loop.** With a pattern such as `packages/**`, a link to a folder outside the project, or to a parent folder, made the workspace walk descend through it. A parent link listed the same packages again under ever longer paths until the path got too long. The walk now descends through a link only when its target is inside the project and is not a parent of the link.
