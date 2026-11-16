### Fixed

- **`hardcoded-secret` no longer reports sentences and public URLs.** The entropy check now skips a value of three or more words, such as an error message or a SQL statement under a key that contains `token` or `credential`. It also skips an `http` or `https` URL without user information, a query or a fragment, such as an OAuth token endpoint or a help link. A URL with `user:pass@` or a `?token=` query stays a candidate. Provider token shapes do not use these filters. (Closes [#4610](https://github.com/fallow-rs/fallow/issues/4610))
