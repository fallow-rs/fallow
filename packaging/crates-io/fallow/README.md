# fallow

Codebase intelligence for TypeScript and JavaScript.

This crate installs fallow for TypeScript and JavaScript. To analyze a Rust
workspace, install [`cargo-fallow`](https://crates.io/crates/cargo-fallow) and
run `cargo fallow`.

This crate installs the `fallow` command:

```bash
cargo install fallow
```

The command is the same as the command of
[`fallow-cli`](https://crates.io/crates/fallow-cli). Each release of `fallow`
pins the `fallow-cli` release with the same version.

Both crates install a binary named `fallow`, so install only one of them. To
switch from `fallow-cli`, run `cargo uninstall fallow-cli` first.

Documentation: <https://fallow.tools/docs/>

Source: <https://github.com/fallow-rs/fallow>
