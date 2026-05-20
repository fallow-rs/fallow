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
