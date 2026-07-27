#!/usr/bin/env bash
# Sync npm package.json versions with the Rust workspace version.
# Called by cargo-release as a pre-release hook.
# Arguments: $1 = old version, $2 = new version
set -euo pipefail

VERSION="${2:-$1}"
OLD_VERSION="${1:-}"
ROOT="$(git rev-parse --show-toplevel)"

update_version() {
  node -e "
    const fs = require('fs');
    const pkg = JSON.parse(fs.readFileSync('$1', 'utf8'));
    pkg.version = '$VERSION';
    fs.writeFileSync('$1', JSON.stringify(pkg, null, 2) + '\n');
  "
}

update_optional_deps() {
  node -e "
    const fs = require('fs');
    const pkg = JSON.parse(fs.readFileSync('$1', 'utf8'));
    pkg.version = '$VERSION';
    if (pkg.optionalDependencies) {
      for (const key of Object.keys(pkg.optionalDependencies)) {
        if (key.startsWith('@fallow-cli/') || key === 'fallow-type-aware') {
          pkg.optionalDependencies[key] = '$VERSION';
        }
      }
    }
    fs.writeFileSync('$1', JSON.stringify(pkg, null, 2) + '\n');
  "
}

update_package_lockfile() {
  node -e "
    const fs = require('fs');
    const lockPath = '$1';
    const lock = JSON.parse(fs.readFileSync(lockPath, 'utf8'));
    lock.version = '$VERSION';
    if (lock.packages?.['']) {
      lock.packages[''].version = '$VERSION';
      if (lock.packages[''].optionalDependencies) {
        for (const key of Object.keys(lock.packages[''].optionalDependencies)) {
          if (key.startsWith('@fallow-cli/fallow-node') || key === 'fallow-type-aware') {
            lock.packages[''].optionalDependencies[key] = '$VERSION';
          }
        }
      }
    }
    fs.writeFileSync(lockPath, JSON.stringify(lock, null, 2) + '\n');
  "
}

# Rewrite the version baked into the NAPI-RS-generated index.js. napi-rs
# hardcodes the current package.json version into per-binding checks (driven
# by NAPI_RS_ENFORCE_VERSION_CHECK), and a bare version bump would otherwise
# ship stale strings until someone re-runs `napi build` locally.
# Argument: $1 = path to index.js
update_napi_index_version() {
  node -e "
    const fs = require('fs');
    const path = '$1';
    const oldVersion = '$OLD_VERSION';
    const newVersion = '$VERSION';
    if (!oldVersion || oldVersion === newVersion) {
      process.exit(0);
    }
    const escape = (v) => v.replace(/[.*+?^\${}()|[\]\\\\]/g, '\\\\\$&');
    let contents = fs.readFileSync(path, 'utf8');
    const quoted = new RegExp(\"'\" + escape(oldVersion) + \"'\", 'g');
    contents = contents.replace(quoted, \"'\" + newVersion + \"'\");
    const messageRe = new RegExp('expected ' + escape(oldVersion) + ' but got', 'g');
    contents = contents.replace(messageRe, 'expected ' + newVersion + ' but got');
    fs.writeFileSync(path, contents);
  "
}

# Update main fallow package (version + optionalDependencies)
update_optional_deps "$ROOT/npm/fallow/package.json"
echo "  Updated fallow/package.json → $VERSION"

# Update Node bindings package (version + optionalDependencies)
update_optional_deps "$ROOT/crates/napi/package.json"
echo "  Updated crates/napi/package.json → $VERSION"

update_version "$ROOT/tools/type-aware-sidecar/package.json"
echo "  Updated tools/type-aware-sidecar/package.json → $VERSION"

if [ -f "$ROOT/tools/type-aware-sidecar/package-lock.json" ]; then
  update_package_lockfile "$ROOT/tools/type-aware-sidecar/package-lock.json"
  echo "  Updated tools/type-aware-sidecar/package-lock.json → $VERSION"
fi

if [ -f "$ROOT/crates/napi/package-lock.json" ]; then
  update_package_lockfile "$ROOT/crates/napi/package-lock.json"
  echo "  Updated crates/napi/package-lock.json → $VERSION"
fi

if [ -f "$ROOT/crates/napi/index.js" ]; then
  update_napi_index_version "$ROOT/crates/napi/index.js"
  echo "  Rewrote version strings in crates/napi/index.js → $VERSION"
fi

# Update platform-specific npm packages
for pkg in "$ROOT"/npm/*/package.json; do
  case "$pkg" in
    */fallow/package.json) continue ;; # Already handled above
  esac
  [ -f "$pkg" ] || continue
  update_version "$pkg"
done

echo "  Updated all platform package versions → $VERSION"
