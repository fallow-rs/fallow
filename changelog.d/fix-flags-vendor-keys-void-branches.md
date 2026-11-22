### Fixed

- **`fallow flags --flag-state` rejects two keys that name one flag.**
  With `flags.vendorKeyPrefix` set to `app.`, the keys `checkout` and `app.checkout` both name the flag `checkout`. The report then used the state of the last key and dropped the other. The command now exits with code 2 and `FALLOW_FLAG_STATE_INVALID`, and the message names the two keys.
- **A ternary arm with `void <call>` is not an empty flag branch.**
  `flag ? void track() : null` runs `track()` when the flag is on, so the flag does something. Only `void` of a literal or of `undefined`, such as `void 0`, now counts as an empty arm. The parse cache version changes, so the first run after the upgrade parses each file again.
