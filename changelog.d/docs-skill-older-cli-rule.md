### Changed

- **The `fallow` skill says what to do when the installed CLI is older than the skill.** A plugin can deliver a newer skill than the installed CLI, and the skill can then name a flag that this CLI rejects. Agent rule 17 tells the reader that an `unexpected argument` error means a version difference: report the version, tell the user to update, and continue without the flag. (Closes [fallow-skills#235](https://github.com/fallow-rs/fallow-skills/issues/235))
