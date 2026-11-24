### Fixed

- **A dependency that a config requires now counts as used.** An Expo app config that sets `userInterfaceStyle` needs `expo-system-ui`. Fallow reported the listed package as an unused dependency, and the removed package as unlisted. A listed required dependency is now used. The unlisted finding stays when no manifest lists the package.
