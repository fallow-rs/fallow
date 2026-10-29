### Changed

- **`fallow flags` gives `medium` confidence to more generic SDK names when the file has no flag import.** The names are `isOn`, `isOff`, `useConfig`, `variation`, `getVariant` and `hasFeature`. Other code also uses these names, for example a product `getVariant('oak-30x40')` or a switch `lamp.isOn()`. Before, each call with a string argument was an `sdk_call` with `high` confidence. A file that imports a flag SDK or a flag module keeps `high`. A name in `flags.sdkPatterns` also stays `high`. `useExperiment` and the other vendor names stay `high`.

### Fixed

- **An import such as `country-flag-icons` or `@/features/checkout/api` no longer confirms a flag file.** Before, the words `flag`, `feature` and `toggle` anywhere in an import source made a generic SDK call `high`. Now one path segment must name a flag vendor or flags, for example `@unleash/proxy-client-react`, `flags/next`, `@/lib/flags` or `./featureFlags`. An import such as `./ThemeToggle` also no longer confirms a flag file.

- **`fallow flags` no longer reports a call to a project function with a generic SDK name.** When a file declares `isEnabled`, or imports `getVariant` from a module of the project, and imports no flag SDK or flag module, a call such as `getVariant('oak-30x40')` is not a flag. An import from a flag module of the project, such as `./featureFlags`, `./flags.server` or `./flagUtils`, still reports with `high`. An import from a module such as `./featureGate`, `@/lib/features` or `./experiments` still reports with `medium`. The extraction cache version changes, so the first run after the update parses all files again.
