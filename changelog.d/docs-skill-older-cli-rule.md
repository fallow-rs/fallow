### Changed

- **The `fallow` skill says what to do when the installed CLI is older than the skill.** A plugin can deliver a newer skill than the installed CLI, and the skill can then name a flag, a value or a subcommand that this CLI rejects. Agent rule 17 tells the reader that an `unexpected argument` error, an `invalid value` error, or an unknown subcommand means a version difference: report the version, tell the user to update, and continue without the argument. (Closes [fallow-skills#235](https://github.com/fallow-rs/fallow-skills/issues/235))
