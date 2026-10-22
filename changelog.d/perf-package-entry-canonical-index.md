### Performance

- **Public API entry resolution no longer resolves every module path again for each missed package entry.**
  A package entry that exists on disk but is not in the module graph, such as
  an extensionless `bin` script, made Fallow resolve the real path of each
  module in the package. Fallow did this again for each candidate path that it
  probed. Fallow now builds the canonical module index one time for each
  package and resolves each parent directory one time. On a large project,
  this removes several seconds from `fallow dead-code` and from the
  `--type-aware` refinement. The results do not change.
