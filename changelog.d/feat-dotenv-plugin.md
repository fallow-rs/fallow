### Added

- **Built-in dotenv CLI plugin support.** `fallow migrate` now maps the
  `dotenv` plugin key. Scripts invoking `dotenvx` credit `@dotenvx/dotenvx`,
  and `dotenvx run -- <command>` credits the wrapped command, as
  `varlock run --` already does.
  (Closes [#3762](https://github.com/fallow-rs/fallow/issues/3762))
