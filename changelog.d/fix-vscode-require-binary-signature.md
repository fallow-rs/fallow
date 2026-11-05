### Security

- **The VS Code extension requires an Ed25519 signature for every binary that it downloads.** Before this change, the extension accepted a release asset without a `.sig` file when the asset matched the digest of the GitHub API. A person who can upload release assets also controls that digest, so the check did not stop an unsigned upload. Now the extension refuses an asset without a signature before it downloads the binary. Every release since v2.45.1 signs each binary, so a normal install does not change.
  - An installed `fallow` or `fallow-lsp` binary that has only a `.sha256` marker and no signature is deleted and downloaded again on the next start.
  - When a release asset has no signature, the error tells you to set `fallow.lspPath` to a `fallow-lsp` binary that you trust, then reload the window.
