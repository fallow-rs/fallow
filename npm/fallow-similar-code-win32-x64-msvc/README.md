# @fallow-cli/fallow-similar-code-win32-x64-msvc

This package contains the Windows x64 binary of [`fallow-similar-code`](https://www.npmjs.com/package/fallow-similar-code), the local embedding sidecar for fallow similar-code analysis. Fallow is codebase intelligence for TypeScript and JavaScript: unused code, duplication, circular dependencies, complexity and architecture checks.

## Install

Do not install this package directly. Install `fallow-similar-code`. Your package manager then installs the platform package for your system from the `optionalDependencies` of `fallow-similar-code`.

```bash
npm install --save-dev fallow fallow-similar-code
```

## Verification

Before the first run, `fallow-similar-code` checks the Ed25519 signature (`fallow-similar-code.sig`) and the SHA-256 digest (`fallowDigests` in `package.json`) of the binary. When a check fails, it does not run the binary.

## Links

- Documentation: https://fallow.tools/docs/
- Source code: https://github.com/fallow-rs/fallow
- Security policy: https://github.com/fallow-rs/fallow/blob/main/SECURITY.md

## License

MIT. See `LICENSE`.
