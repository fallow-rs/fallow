### Added

- **A CodeRabbit plugin and a GNOME Shell extension plugin.** The `coderabbit` plugin keeps `.coderabbit.config.ts` in use when the project depends on `@coderabbitai/config`. The `gnome-shell-extension` plugin makes `extension.js` and `prefs.js` entry points in each folder whose `metadata.json` has the `uuid` and `shell-version` keys that GNOME Shell requires. A `metadata.json` of another tool seeds nothing.
  (Refs [#4912](https://github.com/fallow-rs/fallow/issues/4912))

### Fixed

- **An Expo app no longer needs a config entry for its fingerprint config, its font packages and its native-only modules.** The Expo plugins keep `fingerprint.config.js` in use. A font file path in the app config, such as `"@expo-google-fonts/inter/400Regular/Inter_400Regular.ttf"`, credits the package that holds the file. A dependency with an `expo-module.config.json` file and no JavaScript entry is a native module that Expo autolinking adds to the build, so fallow no longer reports it as unused. Both credits apply only to a dependency that the `package.json` of the app declares and that is installed.
  (Refs [#4912](https://github.com/fallow-rs/fallow/issues/4912))
