#!/usr/bin/env bash
# Sync every version-bearing packaging surface with the Rust workspace version.
# Called by cargo-release as a pre-release hook.
#
# Usage: sync-npm-versions.sh [--skip-napi] <old version> [<new version>]
#
# --skip-napi leaves crates/napi untouched. The release commit needs that:
# `npm ci --omit=optional` runs there before the tag publishes the new platform
# packages, so bumping crates/napi early leaves the lockfile unresolvable.
# crates/napi catches up after publish. FALLOW_SYNC_SKIP_NAPI=1 sets the same
# mode from the environment.
set -euo pipefail

SKIP_NAPI="${FALLOW_SYNC_SKIP_NAPI:-0}"
ARGS=()
while [ "$#" -gt 0 ]; do
  case "$1" in
    --skip-napi) SKIP_NAPI=1 ;;
    --)
      shift
      while [ "$#" -gt 0 ]; do
        ARGS+=("$1")
        shift
      done
      break
      ;;
    -*)
      echo "sync-npm-versions.sh: unknown option: $1" >&2
      exit 2
      ;;
    *) ARGS+=("$1") ;;
  esac
  shift
done

OLD_VERSION="${ARGS[0]:-}"
VERSION="${ARGS[1]:-$OLD_VERSION}"
if [ -z "$VERSION" ]; then
  echo "sync-npm-versions.sh: expected a version argument" >&2
  exit 2
fi
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
        if (key.startsWith('@fallow-cli/') || key === 'fallow-type-aware' || key === 'fallow-similar-code') {
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
          if (key.startsWith('@fallow-cli/fallow-node') || key === 'fallow-type-aware' || key === 'fallow-similar-code') {
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

update_similar_code_cargo_version() {
  node -e "
    const fs = require('fs');
    const manifestPath = '$ROOT/tools/similar-code-sidecar/Cargo.toml';
    let manifest = fs.readFileSync(manifestPath, 'utf8');
    manifest = manifest.replace(
      /(name = \"fallow-similar-code-sidecar\"\nversion = \")[^\"]+/,
      '\$1$VERSION',
    );
    fs.writeFileSync(manifestPath, manifest);
    const lockPath = '$ROOT/tools/similar-code-sidecar/Cargo.lock';
    if (fs.existsSync(lockPath)) {
      let lock = fs.readFileSync(lockPath, 'utf8');
      lock = lock.replace(
        /(name = \"fallow-similar-code-sidecar\"\nversion = \")[^\"]+/,
        '\$1$VERSION',
      );
      fs.writeFileSync(lockPath, lock);
    }
  "
}

update_fallow_crate_version() {
  node -e "
    const fs = require('fs');
    const manifestPath = '$ROOT/packaging/crates-io/fallow/Cargo.toml';
    let manifest = fs.readFileSync(manifestPath, 'utf8');
    manifest = manifest
      .replace(/(name = \"fallow\"\nversion = \")[^\"]+/, '\$1$VERSION')
      .replace(/^(fallow-cli = \"=)[^\"]+/m, '\$1$VERSION');
    fs.writeFileSync(manifestPath, manifest);
  "
}

# Update main fallow package (version + optionalDependencies)
update_optional_deps "$ROOT/npm/fallow/package.json"
echo "  Updated fallow/package.json → $VERSION"

update_optional_deps "$ROOT/npm/fallow-similar-code/package.json"
echo "  Updated fallow-similar-code/package.json → $VERSION"

update_similar_code_cargo_version
echo "  Updated similar-code sidecar Cargo version → $VERSION"

update_fallow_crate_version
echo "  Updated fallow crate version and fallow-cli pin → $VERSION"

# Update Node bindings package (version + optionalDependencies)
if [ "$SKIP_NAPI" = "1" ]; then
  echo "  Skipped crates/napi (--skip-napi)"
else
  update_optional_deps "$ROOT/crates/napi/package.json"
  echo "  Updated crates/napi/package.json → $VERSION"

  if [ -f "$ROOT/crates/napi/package-lock.json" ]; then
    update_package_lockfile "$ROOT/crates/napi/package-lock.json"
    echo "  Updated crates/napi/package-lock.json → $VERSION"
  fi

  if [ -f "$ROOT/crates/napi/index.js" ]; then
    update_napi_index_version "$ROOT/crates/napi/index.js"
    echo "  Rewrote version strings in crates/napi/index.js → $VERSION"
  fi
fi

update_version "$ROOT/tools/type-aware-sidecar/package.json"
echo "  Updated tools/type-aware-sidecar/package.json → $VERSION"

if [ -f "$ROOT/tools/type-aware-sidecar/package-lock.json" ]; then
  update_package_lockfile "$ROOT/tools/type-aware-sidecar/package-lock.json"
  echo "  Updated tools/type-aware-sidecar/package-lock.json → $VERSION"
fi

# Update platform-specific npm packages
for pkg in "$ROOT"/npm/*/package.json; do
  case "$pkg" in
    */fallow/package.json|*/fallow-similar-code/package.json) continue ;; # Already handled above
  esac
  [ -f "$pkg" ] || continue
  update_version "$pkg"
done

echo "  Updated all platform package versions → $VERSION"

# The MCP Registry server card carries the version twice and the published
# skill examples carry it under two different keys, so both go through one
# writer that the CI gate reuses in --check mode.
node "$ROOT/scripts/sync-emitted-versions.mjs" --version "$VERSION"
