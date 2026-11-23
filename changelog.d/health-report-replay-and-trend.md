### Fixed

- **Saved health reports render Markdown, GitHub summaries, and GitHub annotations outside the original checkout.** These formats use the saved report even when the new directory has no CODEOWNERS file or workspace packages.
- **The health trend error gives a repair command for the current analysis mode.** The command keeps the project root and the selected snapshot path. If the path is outside the allowed write directories, the error shows how to save and compare a snapshot in the project.
