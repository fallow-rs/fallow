### Security

- **The VS Code extension bundles minimatch 11.0.3.** The language client requested minimatch 10, which has a denial of service through glob pattern amplification (GHSA-g3hv-75wg-m249). The extension only matches globs that the fallow language server sends.
