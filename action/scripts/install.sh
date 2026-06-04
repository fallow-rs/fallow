#!/usr/bin/env bash
set -eo pipefail

# Install fallow binary via npm.
# Optional env: FALLOW_VERSION, INPUT_ROOT, FALLOW_INSTALL_DRY_RUN.

trim() {
  local value="$1"
  value="${value#"${value%%[![:space:]]*}"}"
  value="${value%"${value##*[![:space:]]}"}"
  printf '%s' "$value"
}

is_safe_version_spec() {
  local spec
  spec="$(trim "$1")"
  if [ "$spec" = "latest" ]; then
    return 0
  fi
  local start_re='^[0-9xX*~^<>=]'
  local safe_re='^[0-9A-Za-z.*~^<>=| -]+$'
  # Accept semver versions and ranges, while rejecting protocols, paths, and
  # package aliases such as file:, link:, workspace:, git URLs, or /tmp/foo.
  [[ "$spec" =~ $start_re ]] &&
    [[ "$spec" =~ $safe_re ]] &&
    [[ ! "$spec" =~ : ]] &&
    [[ ! "$spec" =~ / ]] &&
    [[ ! "$spec" =~ [[:space:]]-[A-Za-z] ]]
}

is_exact_version() {
  [[ "$1" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-.][a-zA-Z0-9.]+)?$ ]]
}

project_fallow_spec() {
  local package_json="$1/package.json"
  if [ ! -f "$package_json" ]; then
    return 0
  fi

  node - "$package_json" <<'NODE'
const fs = require("node:fs");
const packageJson = process.argv[2];
const pkg = JSON.parse(fs.readFileSync(packageJson, "utf8"));
for (const section of ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies"]) {
  const spec = pkg[section]?.fallow;
  if (typeof spec === "string" && spec.trim()) {
    console.log(spec.trim());
    process.exit(0);
  }
}
NODE
}

requested_version="$(trim "${FALLOW_VERSION:-}")"
root="${INPUT_ROOT:-.}"
project_spec="$(project_fallow_spec "$root" 2>/dev/null || true)"
project_spec="$(trim "$project_spec")"
install_spec=""

# version_source records WHERE the resolved CLI version came from, so a later
# verification failure can name the knob to turn (the CLI version is distinct
# from the Action ref, the exact confusion behind #944).
version_source=""

if [ -n "$requested_version" ]; then
  install_spec="$requested_version"
  version_source="the action 'version' input"
  echo "::notice::Using fallow version from action input: ${install_spec}"
elif [ -n "$project_spec" ]; then
  if is_safe_version_spec "$project_spec"; then
    install_spec="$project_spec"
    version_source="the fallow dependency in ${root}/package.json"
    echo "::notice::Using fallow version from ${root}/package.json: ${install_spec}"
  else
    echo "::warning::Ignoring unsupported fallow package.json spec '${project_spec}'. Use a semver version or range, or set the action 'version' input explicitly."
    install_spec="latest"
    version_source="the latest published release"
  fi
else
  install_spec="latest"
  version_source="the latest published release"
fi

if ! is_safe_version_spec "$install_spec"; then
  echo "::error::Invalid version specifier: ${install_spec}. Use 'latest' or a semver version/range like '2.52.2' or '^2.52.0'."
  exit 2
fi

if [ "$install_spec" = "latest" ]; then
  install_arg="fallow"
else
  install_arg="fallow@${install_spec}"
fi

if [ "${FALLOW_INSTALL_DRY_RUN:-}" = "true" ]; then
  echo "DRY RUN: npm install -g --ignore-scripts ${install_arg}"
  exit 0
fi

npm install -g --ignore-scripts "$install_arg"

# Verify with code bundled in the checked-out Action, not code from the
# installed npm package. This keeps CI runners from executing untrusted package
# lifecycle scripts before the binary signature + digest checks complete.
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
action_root="${GITHUB_ACTION_PATH:-$(cd "$script_dir/../.." && pwd)}"
verify_script="$action_root/npm/fallow/scripts/verify-binary.js"
global_root="$(npm root -g)"
global_fallow_root="$global_root/fallow"
if [ ! -f "$verify_script" ]; then
  echo "::error::Verifier script not found at ${verify_script}; cannot verify fallow binaries"
  exit 1
fi

# The actually-installed CLI version, read from the global package manifest
# rather than `fallow --version` (which re-runs the lazy verify and would fail
# again on the same binary). Best-effort; used only for the failure context.
# Strip CR/LF before this value lands in a `::error::` workflow command: it is
# read from a manifest that may be tampered with (this context only renders on a
# verification FAILURE), and an embedded newline could otherwise inject a
# spoofed workflow command. `|| true` keeps the pipeline from tripping set -e.
installed_fallow_version="$(node -p "require('${global_fallow_root}/package.json').version" 2>/dev/null | tr -d '\r\n' || true)"
[ -n "$installed_fallow_version" ] || installed_fallow_version="unknown"

if ! ACTION_VERIFY_SCRIPT="$verify_script" FALLOW_VERIFY_RESOLVE_FROM="$global_fallow_root" node <<'NODE'
(async () => {
  const { verifyInstalled, SKIP_ENV } = require(process.env.ACTION_VERIFY_SCRIPT);
  const result = await verifyInstalled({ resolveFrom: process.env.FALLOW_VERIFY_RESOLVE_FROM });
  if (result.skipped) {
    console.log('::warning::Binary verification skipped because ' + SKIP_ENV + ' is set. Only use this when deliberately replacing the published binary.');
    process.exit(0);
  }
  if (!result.ok) {
    const where = result.binary ? ' ' + result.binary : '';
    console.error('::error::fallow binary verification failed' + where + ' (' + result.code + '): ' + result.message);
    process.exit(1);
  }
  console.log('Verified Ed25519 signatures and SHA-256 digests on fallow binaries (package ' + result.package + '@' + result.version + ')');
})().catch((err) => {
  console.error('::error::fallow binary verification failed (internal-error): ' + err.message);
  process.exit(1);
});
NODE
then
  # The verifier above printed the version-aware fix (bump the pin for a
  # pre-signing version, or treat a missing signature on a signed-era package
  # as tampering). Add the locate-the-knob context: which version was installed
  # and from where, since the Action ref is a different knob from the CLI
  # version. Neutral wording so it stays correct for both failure causes.
  echo "::error::Verification ran against fallow ${installed_fallow_version}, installed from ${version_source}. The Action ref (${GITHUB_ACTION_REF:-see your workflow}) selects the Action code, not the CLI version. Apply the recommended fix in the verification error above."
  exit 1
fi

installed_version="$(fallow --version 2>/dev/null || echo 'unknown version')"
echo "Installed fallow ${installed_version}"

if [ -z "$requested_version" ] && [ -n "$project_spec" ] && is_exact_version "$project_spec"; then
  installed_semver="$(printf '%s\n' "$installed_version" | grep -Eo '[0-9]+\.[0-9]+\.[0-9]+([-.][a-zA-Z0-9.]+)?' | head -n 1 || true)"
  if [ -n "$installed_semver" ] && [ "$installed_semver" != "$project_spec" ]; then
    echo "::warning::Installed fallow ${installed_semver}, but ${root}/package.json pins ${project_spec}. Set the action 'version' input or align package.json to keep local and CI results comparable."
  fi
fi
