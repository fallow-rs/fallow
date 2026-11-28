### Fixed

- **The import of an old in-repo Impact store keeps the user decisions unset.** When no user store exists, fallow imports the history and the counters of `.fallow/impact.json`. The import no longer copies `enabled`, `onboarding_declined`, `explicit_decision` or the digest time, so a file in the checkout cannot turn Impact tracking on or hide the opt-in question. A `.fallow/impact.json` that Git tracks is not imported.
