### Fixed

- **Small config files with repeated references no longer make a run take minutes or gigabytes of memory.** Fallow now bounds four readers that expanded such references without a limit:
  - YAML aliases in `pnpm-workspace.yaml`, `pnpm-lock.yaml` and other YAML configs. A file whose aliases expand to more than 256 MiB of copies is reported as invalid YAML, as other invalid YAML is.
  - Spreads of a constant in the `resolve.alias` value of a bundler config. Fallow resolves each name once, and keeps at most 512 aliases for one value.
  - Spreads of a constant in the options of a Module Federation plugin call. Fallow resolves each name once.
  - Calls of a package.json script with arguments, such as `npm run lint -- src`. The commands of one package now share one budget of 256 KiB of expanded script text, for both the dependency credit and the file references of the scripts. A call after that budget is not expanded.

  Real configs stay far below these limits, so the analysis results of real projects do not change.
