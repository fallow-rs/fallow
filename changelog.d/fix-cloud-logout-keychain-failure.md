### Fixed

- **`fallow cloud logout` reports an incomplete logout when the keychain does not confirm the delete.**
  A denied or timed-out keychain prompt counted as "no stored key", so the command printed `No stored API key` or `Logged out` while the key could still be in the keychain. The command now exits with code 2 and says that the logout is incomplete. It still removes the key from the credentials file.
