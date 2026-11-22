### Fixed

- **A recursive workspace pattern no longer follows a link out of the project or into a loop.** With a pattern such as `packages/**`, a link to a folder outside the project, to a parent folder, or two links between sibling folders made the workspace walk descend through them. A loop listed the same packages again under ever longer paths until the path got too long. The walk now enters each real folder once, and it does not enter a link whose target is outside the project or inside the folder that it walks. A link to a package is still listed.
