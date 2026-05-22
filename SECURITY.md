# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability in fallow, please report it responsibly via [GitHub's private vulnerability reporting](https://github.com/fallow-rs/fallow/security/advisories/new) instead of opening a public issue.

You should receive a response within 48 hours. Please include:

- A description of the vulnerability
- Steps to reproduce it
- Any relevant version or configuration information

## Scope

fallow is a static analysis tool that reads source files and `package.json`. It does not execute user code, make network requests, or modify files (except `fallow fix`, which only edits files in the analyzed project).

## Threat model

The primary security boundary is the project root passed via `--root` (or the discovered config's directory). fallow walks files under that root and reads `package.json`, source files, lockfiles, and CI configs found within it.

Config-sourced glob patterns (`entry`, `ignorePatterns`, `dynamicallyLoaded`, `duplicates.ignore`, `health.ignore`, `overrides[].files`, `ignoreExports[].file`, `ignoreCatalogReferences[].consumer`, `boundaries.zones[].{patterns, root, autoDiscover}`) are validated against absolute paths, `..` traversal segments, and invalid glob syntax at config load time. The same validation applies to every glob-bearing field on inline `framework[]` plugin definitions and on external plugin files discovered from `.fallow/plugins/`, root-level `fallow-plugin-*.{toml,json,jsonc}`, or paths listed in the `plugins:` config field, including patterns nested inside `detection` combinators (`all`, `any`). Invalid patterns cause `fallow` to exit with code 2 before walking the filesystem, so a malicious `.fallowrc.json` or plugin file shipped in a PR cannot smuggle absolute or traversal globs into a CI run. See issue [#463](https://github.com/fallow-rs/fallow/issues/463) for the original report.

On `fallow-rs/fallow`'s own GitHub Actions setup, the `approval_policy: first_time_contributors` setting requires maintainer approval before a first-time contributor's PR runs CI, which further narrows the realistic attack window. Self-hosted forks should configure a similar approval policy when running `fallow` on untrusted PR content.

## Binary distribution and verification

Every fallow release publishes per-platform CLI, LSP, and MCP binaries via three channels (the GitHub Release, the `@fallow-cli/*` npm platform packages, and the bundled `fallow-rs/fallow@v2` GitHub Action). At release time the `build` job in `.github/workflows/release.yml` signs each binary with the workflow's Ed25519 private key (`ED25519_BINARY_SIGNING_PRIVATE_KEY` repo secret), uploads the resulting `.sig` files alongside the binaries, and publishes npm tarballs with `npm publish --provenance --ignore-scripts`. The same workflow computes a SHA-256 digest of every platform binary and writes it into the platform package's `package.json` under a `fallowDigests` field, so npm and Action installs verify binary bytes locally without a network round-trip.

The matching public key is `34 bytes of SPKI DER header + 32 raw bytes of Ed25519 public key`. The 32-byte raw key is hardcoded into every consumer (the VS Code extension at `editors/vscode/src/download.ts`, the npm wrapper at `npm/fallow/scripts/verify-binary.js`) so the Ed25519 layer of verification works fully offline and cannot be silently downgraded by network-path tampering. The SHA-256 layer reads the embedded `fallowDigests` field from the platform package's `package.json`. If the field is absent (older platform packages published before v2.78.1), the verifier falls back to fetching the platform asset's `digest` field from `https://api.github.com/repos/fallow-rs/fallow/releases/tags/v<version>`; this fallback can fail on shared-IP CI runners that exceed GitHub's 60 req/hr unauthenticated rate limit (issue #597), which is why steady-state verification uses the embedded path.

**Public key fingerprint (raw 32-byte Ed25519, hex):**

```
834e6fd77333e6eedf779347c710acb403d2d8234d559f5ed7c87e552ade0bd1
```

You can copy this value out-of-band (a release blog post, this file at a tag you trust, a Git commit you trust) and compare it against the embedded copy in any version of fallow you have installed.

### Verification surfaces

| Channel | When verification runs | What it verifies | Failure mode |
|:--------|:-----------------------|:-----------------|:-------------|
| VS Code extension | After downloading the binary from the GitHub Release | Ed25519 signature over the binary bytes; SHA-256 fallback when no `.sig` is present | Refuses to launch and deletes the partial download |
| `npm install fallow` (postinstall) | After the platform package is resolved | Ed25519 signature over each of `fallow`, `fallow-lsp`, `fallow-mcp` in the resolved `@fallow-cli/<platform>` package, then SHA-256 of the binary bytes against the platform package's `fallowDigests` field (with the GitHub Release `asset.digest` field as a fallback for older packages) | Aborts the install with exit code 1 and a `fallow: binary verification failed: ...` message |
| `fallow-rs/fallow@v2` GitHub Action installer | After `npm install -g --ignore-scripts fallow@<spec>` | Same as above, but the verifier code is loaded from the checked-out Action tree rather than the installed package so a tampered postinstall cannot self-validate | Aborts the action step with a `::error::` annotation |

### Out-of-band verification recipe

To verify a binary manually, download both the binary and its `.sig` from a GitHub Release (e.g. `fallow-aarch64-apple-darwin` + `fallow-aarch64-apple-darwin.sig`) and run the workflow's verification script with the public key set in env:

```sh
ED25519_BINARY_SIGNING_PUBLIC_KEY=g05v13Mz5u7fd5NHxxCstAPS2CNNVZ9e18h+VSreC9E= \
  node .github/scripts/verify-binary.mjs fallow-aarch64-apple-darwin fallow-aarch64-apple-darwin.sig
```

The base64 form of the public key above (`g05v13Mz5u7fd5NHxxCstAPS2CNNVZ9e18h+VSreC9E=`) decodes to the same 32 bytes shown in the fingerprint section.

For the SHA-256 half, compare the local binary hash with the digest embedded in the matching `@fallow-cli/<platform>` package's `package.json`:

```sh
shasum -a 256 node_modules/@fallow-cli/linux-x64-gnu/fallow
node -p 'require("@fallow-cli/linux-x64-gnu/package.json").fallowDigests.fallow'
```

Both lines should print the same hex digest (the second carries a `sha256:` prefix). For platform packages published before v2.78.1 that do not yet ship `fallowDigests`, compare against the GitHub Release asset digest instead:

```sh
gh release view v2.76.0 --repo fallow-rs/fallow --json assets \
  --jq '.assets[] | select(.name=="fallow-aarch64-apple-darwin") | .digest'
```

### The `FALLOW_SKIP_BINARY_VERIFY` escape hatch

Set `FALLOW_SKIP_BINARY_VERIFY=1` (or `true` or `yes`) in the environment to skip Ed25519 and SHA-256 verification during `npm install` and during the GitHub Action installer step. This emits a warning so the skip is visible in CI logs.

Use this ONLY when you deliberately replace the published binary, for example:

- You build fallow from source and patch the binary into the platform package after install.
- You mirror npm through a private registry that re-signs or repacks artifacts.
- You run fallow inside an airgapped environment with a locally-built binary.

Do NOT set this flag in regular CI configurations or on machines that are expected to consume the upstream release. An attacker who can set environment variables on your install host can use the same flag to bypass verification; the flag exists for legitimate replacement workflows, not as a noise-reducer.

### Reporting binary tampering

If `npm install fallow` or the `fallow-rs/fallow` action ever aborts with `binary verification failed` on a fresh, unmodified install, do not ignore it. Report it via the [private vulnerability reporting link](https://github.com/fallow-rs/fallow/security/advisories/new) above and include the full error message and the platform package version. False positives on this path are rare; a sustained failure on a clean install is treated as a P0 supply-chain incident.
