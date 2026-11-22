### Fixed

- **The Impact store expiry keeps the decision of a project.**
  With `FALLOW_IMPACT_STORE_MAX_AGE_DAYS` set, the expiry deleted each old project store. A project with `fallow impact disable` has no new records, so its store aged out and the project fell back to the user-global default. The expiry now keeps an explicit enable or disable and a declined opt-in, and removes only the history. A store without a decision is still removed.
