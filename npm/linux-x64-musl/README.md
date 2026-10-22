# @fallow-cli/linux-x64-musl

This package contains the Linux x64 (musl) binaries of the [`fallow`](https://www.npmjs.com/package/fallow) CLI. Fallow is codebase intelligence for TypeScript and JavaScript: unused code, duplication, circular dependencies, complexity and architecture checks.

## Install

Do not install this package directly. Install `fallow`. Your package manager then installs the platform package for your system from the `optionalDependencies` of `fallow`.

```bash
npm install --save-dev fallow
```

## Verification

Before the first run, the `fallow` package checks the Ed25519 signature (`fallow.sig`) and the SHA-256 digest (`fallowDigests` in `package.json`) of the binary. When a check fails, `fallow` does not run the binary.

## Links

- Documentation: https://fallow.tools/docs/
- Source code: https://github.com/fallow-rs/fallow
- Security policy: https://github.com/fallow-rs/fallow/blob/main/SECURITY.md

## License

MIT. See `LICENSE`.
