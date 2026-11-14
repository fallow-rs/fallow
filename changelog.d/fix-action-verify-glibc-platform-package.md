### Fixed

- **The GitHub Action no longer fails its install step on a glibc Linux runner.**
  Since 3.43.0, npm installs only the platform package for the libc of the
  host. The Action runs the binary verifier from the Action checkout, where
  `detect-libc` is not installed, so the verifier treated a glibc host as musl
  and stopped with `platform package @fallow-cli/linux-x64-musl not installed`.
  The verifier now also looks for `detect-libc` in the installed `fallow`
  package. (Closes [#4591](https://github.com/fallow-rs/fallow/issues/4591))
