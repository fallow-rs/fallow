### Fixed

- **`fallow impact` commands report a failed save.** `fallow impact enable`,
  `disable`, `default on|off`, `reset` and `fallow init --decline` printed
  success when the change could not be saved, for example in a read-only
  config folder. They now print the error and exit with code 2.
- **`fallow impact reset` stays reset.** After a reset, the next run imported
  the history of an old in-repo `.fallow/impact.json` again. A marker file next
  to the store now stops a second import. `reset --all` keeps the lock files
  and the markers.
- **A damaged Impact store is not overwritten.** A store that could not be
  parsed was replaced by an empty store on the next routine run. A run, the
  digest, `fallow impact disable` and `fallow init --decline` now leave the file
  unchanged and name the next step. `fallow impact enable` still replaces a
  store that does not parse.
- **`fallow impact --all` skips a store from a newer fallow.** The cross-project
  view read a store that a newer fallow wrote as a current store. It now counts
  that store as unreadable.
