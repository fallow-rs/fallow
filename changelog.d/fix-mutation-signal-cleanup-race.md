### Fixed

- **A Ctrl+C during `fallow mutation` deletes the companion temp directory also when the companion exit and the signal cleanup overlap.** The directory now stays registered for the signal cleanup until its owner has deleted it, and the cleanup tries again when the owner deletes the same directory at the same time.
