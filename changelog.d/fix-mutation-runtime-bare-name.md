### Fixed

- **`fallow mutation --runtime node` finds `node` on `PATH`.** Before, a bare name such as `node` resolved as a file in the working directory, so the run stopped with `FALLOW_MUTATION_RUNTIME_NOT_FOUND` and "`<dir>/node` is not a file". Only an absolute path worked. Now a name without a path separator is a `PATH` lookup, as in a shell. A path with a separator, such as `./bin/node`, stays a file path relative to the working directory.
