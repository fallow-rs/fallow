### Fixed

- **The setup hint of a bare `fallow` run now names `npx fallow` when `npx` started fallow.** Before, the hint said to run `fallow init`. After a `npx fallow` run, that command usually fails with `fallow: command not found`, because `npx` does not put fallow on `PATH`. The hint reads the `npm_command` variable that `npx` and `npm exec` set to `exec`. The tip after `fallow impact enable` uses the same rule for `fallow init --hooks`. Other runs still show `fallow init`. The JSON next steps do not change.
