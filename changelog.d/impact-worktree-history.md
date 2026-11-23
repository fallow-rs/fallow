### Fixed

- **A passing gate clears a pending failure in the same worktree.** Linked worktrees can have failures pending at the same time.
- **Impact matches moved findings one for one.** A new occurrence can cancel one resolution credit for the same symbol and finding kind in that worktree.
- **The Impact containment total keeps growing after the event list fills.** The project report, digest and cross-project report count events that leave the list. For an older store, counting starts with the events it retained.
