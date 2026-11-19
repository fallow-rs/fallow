### Changed

- **The Node bindings and the Rust API read the config once for each call.** A combined or audit call used to load the config for each analysis, so a config edit during a call could give two analyses different settings. The feature flags, mutation and similar-code calls now also check external plugin files, as the other calls do. A config that does not load gives the same error text on each call. The combined result lists each skipped analysis with its reason in the new `skipped` member.
