### Fixed

- **Small config files with repeated references no longer make a run take minutes or gigabytes of memory.** Fallow now bounds four readers that expanded such references without a limit:
  - YAML aliases in `pnpm-workspace.yaml`, `pnpm-lock.yaml` and other YAML configs. A file whose aliases expand to more than 256 MiB of copies is reported as invalid YAML, and a warning names the file. A large config whose entries merge shared defaults is still read in full.
  - Spreads of a constant in the `resolve.alias` value of a bundler config. Fallow resolves each name once, and takes at most 4,096 aliases from the spreads of one value. Aliases that the value writes out in full are all kept.
  - Spreads of a constant in the options of a Module Federation plugin call. Fallow resolves each name once.
  - Calls of a package.json script with arguments, such as `npm run lint -- src`. The commands of one package now share one budget of 256 KiB of expanded script text, for both the dependency credit and the file references of the scripts. A call after that budget is not expanded, and one warning reports it.

  Real configs stay far below these limits, so the analysis results of real projects do not change.
