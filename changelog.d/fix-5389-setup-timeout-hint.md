### Fixed

- **`fallow mutation` suggests a `--setup-timeout` value that is longer than the value that failed.** After a setup timeout, the hint always said "for example 5m", including when the timeout was already 5m or 15m. Now the example is two times the value that failed, with a minimum of 5m. For example, a 5m timeout gives `10m`, and a 15m timeout gives `30m`. The hint of a self-check that timed out uses the same rule, and it starts from the job timeout when that is longer than the setup timeout.
  (Closes [#5389](https://github.com/fallow-rs/fallow/issues/5389))
