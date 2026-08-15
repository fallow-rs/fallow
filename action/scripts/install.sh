#!/usr/bin/env bash
set -eo pipefail

# Install fallow binary via npm, plus the exact-version fallow-type-aware
# sidecar when the project or the action opts into type-aware analysis.
# Optional env: FALLOW_VERSION, INPUT_ROOT, INPUT_CONFIG, INPUT_TYPE_AWARE,
# FALLOW_INSTALL_DRY_RUN.

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

# True when the resolved fallow config enables type-aware analysis. Mirrors
# the CLI's first-match-wins config discovery (.fallowrc.json > .fallowrc.jsonc
# > fallow.toml > .fallow.toml) and respects an explicit config path. The
# audit.typeAware override also counts: audit is the primary CI command, so a
# config that turns type-aware on for audit alone still needs the sidecar.
type_aware_config_enabled() {
  local root="$1" explicit="$2" config_file=""
  if [ -n "$explicit" ]; then
    [ -f "$explicit" ] || return 1
    config_file="$explicit"
  else
    local candidate
    for candidate in .fallowrc.json .fallowrc.jsonc fallow.toml .fallow.toml; do
      if [ -f "$root/$candidate" ]; then
        config_file="$root/$candidate"
        break
      fi
    done
  fi
  [ -n "$config_file" ] || return 1

  case "$config_file" in
    *.toml)
      awk '
        /^[[:space:]]*\[/ { section = $0 }
        /^[[:space:]]*enabled[[:space:]]*=[[:space:]]*true/ && section ~ /\[typeAware\]/ { found = 1 }
        /^[[:space:]]*typeAware[[:space:]]*=[[:space:]]*true/ && section ~ /\[audit\]/ { found = 1 }
        /^[[:space:]]*typeAware\.enabled[[:space:]]*=[[:space:]]*true/ && section == "" { found = 1 }
        END { exit found ? 0 : 1 }
      ' "$config_file"
      ;;
    *)
      node - "$config_file" <<'NODE'
const fs = require("node:fs");
const raw = fs.readFileSync(process.argv[2], "utf8");
// Minimal JSONC strip: comments outside strings only.
let out = "";
let inString = false;
let escaped = false;
for (let i = 0; i < raw.length; i += 1) {
  const char = raw[i];
  const next = raw[i + 1];
  if (inString) {
    out += char;
    if (escaped) escaped = false;
    else if (char === "\\") escaped = true;
    else if (char === '"') inString = false;
    continue;
  }
  if (char === '"') {
    inString = true;
    out += char;
    continue;
  }
  if (char === "/" && next === "/") {
    while (i < raw.length && raw[i] !== "\n") i += 1;
    out += "\n";
    continue;
  }
  if (char === "/" && next === "*") {
    i += 2;
    while (i < raw.length && !(raw[i] === "*" && raw[i + 1] === "/")) i += 1;
    i += 1;
    continue;
  }
  out += char;
}
let config;
try {
  config = JSON.parse(out.replace(/,(\s*[}\]])/g, "$1"));
} catch {
  process.exit(1);
}
const enabled = config?.typeAware?.enabled === true || config?.audit?.typeAware === true;
process.exit(enabled ? 0 : 1);
NODE
      ;;
  esac
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

# Sidecar provisioning decision. 'true' forces it, 'false' skips it, and
# 'auto' (default) reads the project's fallow config, so typeAware-enabled
# projects work in CI without extra workflow wiring (#2107).
type_aware_input="$(trim "${INPUT_TYPE_AWARE:-auto}")"
[ -n "$type_aware_input" ] || type_aware_input="auto"
sidecar_wanted=false
case "$type_aware_input" in
  true)
    sidecar_wanted=true
    echo "::notice::Type-aware sidecar enabled via the action 'type-aware' input"
    ;;
  false)
    ;;
  auto)
    if type_aware_config_enabled "$root" "$(trim "${INPUT_CONFIG:-}")"; then
      sidecar_wanted=true
      echo "::notice::Type-aware sidecar enabled via the project fallow config"
    fi
    ;;
  *)
    echo "::error::Invalid 'type-aware' input: ${type_aware_input}. Use 'true', 'false', or 'auto'."
    exit 2
    ;;
esac

if [ "${FALLOW_INSTALL_DRY_RUN:-}" = "true" ]; then
  echo "DRY RUN: npm install -g --ignore-scripts ${install_arg}"
  if [ "$sidecar_wanted" = "true" ]; then
    if is_exact_version "$install_spec"; then
      echo "DRY RUN: npm install --prefix <tool-dir> --ignore-scripts fallow-type-aware@${install_spec}"
    else
      echo "DRY RUN: npm install --prefix <tool-dir> --ignore-scripts fallow-type-aware@<resolved CLI version>"
    fi
  fi
  exit 0
fi

npm install -g --ignore-scripts "$install_arg"

# Verify with code bundled in the checked-out Action, not code from the
# installed npm package. This keeps CI runners from executing untrusted package
# lifecycle scripts before the binary signature + digest checks complete.
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
action_root="${GITHUB_ACTION_PATH:-$(cd "$script_dir/../.." && pwd)}"
verify_script="$action_root/npm/fallow/scripts/verify-binary.js"
verify_runner="$action_root/action/scripts/verify-installed.mjs"
global_root="$(npm root -g)"
global_fallow_root="$global_root/fallow"
if [ ! -f "$verify_script" ]; then
  echo "::error::Verifier script not found at ${verify_script}; cannot verify fallow binaries"
  exit 1
fi
if [ ! -f "$verify_runner" ]; then
  echo "::error::Verifier runner not found at ${verify_runner}; cannot verify fallow binaries"
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

if ACTION_VERIFY_SCRIPT="$verify_script" FALLOW_VERIFY_RESOLVE_FROM="$global_fallow_root" node "$verify_runner"; then
  :
else
  verify_status=$?
  if [ "$verify_status" -eq 124 ]; then
    exit "$verify_status"
  fi
  # The verifier above printed the version-aware fix (bump the pin for a
  # pre-signing version, or treat a missing signature on a signed-era package
  # as tampering). Add the locate-the-knob context: which version was installed
  # and from where, since the Action ref is a different knob from the CLI
  # version. Neutral wording so it stays correct for both failure causes.
  echo "::error::Verification ran against fallow ${installed_fallow_version}, installed from ${version_source}. The Action ref (${GITHUB_ACTION_REF:-see your workflow}) selects the Action code, not the CLI version. Apply the recommended fix in the verification error above."
  exit "$verify_status"
fi

installed_version="$(fallow --version 2>/dev/null || echo 'unknown version')"
echo "Installed fallow ${installed_version}"

if [ -z "$requested_version" ] && [ -n "$project_spec" ] && is_exact_version "$project_spec"; then
  installed_semver="$(printf '%s\n' "$installed_version" | grep -Eo '[0-9]+\.[0-9]+\.[0-9]+([-.][a-zA-Z0-9.]+)?' | head -n 1 || true)"
  if [ -n "$installed_semver" ] && [ "$installed_semver" != "$project_spec" ]; then
    echo "::warning::Installed fallow ${installed_semver}, but ${root}/package.json pins ${project_spec}. Set the action 'version' input or align package.json to keep local and CI results comparable."
  fi
fi

if [ "$sidecar_wanted" = "true" ]; then
  # The exact-version-match contract: the sidecar must be the SAME version as
  # the CLI the action just resolved, or the native binary rejects it.
  sidecar_version="$installed_fallow_version"
  if ! is_exact_version "$sidecar_version"; then
    sidecar_version="$(printf '%s\n' "$installed_version" | grep -Eo '[0-9]+\.[0-9]+\.[0-9]+([-.][a-zA-Z0-9.]+)?' | head -n 1 || true)"
  fi
  if ! is_exact_version "$sidecar_version"; then
    echo "::error::Cannot determine the installed fallow version, so the exact-version fallow-type-aware sidecar cannot be provisioned. Pin a fallow version via the action 'version' input or package.json."
    exit 1
  fi

  tool_dir="${RUNNER_TEMP:-$(mktemp -d)}/fallow-type-aware"
  mkdir -p "$tool_dir"
  if ! npm install --prefix "$tool_dir" --ignore-scripts "fallow-type-aware@${sidecar_version}"; then
    echo "::error::Failed to install fallow-type-aware@${sidecar_version}. The sidecar must match the resolved CLI version exactly; check that this version exists on npm, or set the action input 'type-aware: false' to skip provisioning."
    exit 1
  fi

  sidecar_bin="$tool_dir/node_modules/fallow-type-aware/fallow-type-aware.mjs"
  if [ ! -f "$sidecar_bin" ]; then
    echo "::error::fallow-type-aware@${sidecar_version} installed, but ${sidecar_bin} does not exist."
    exit 1
  fi
  sidecar_installed_version="$(node -p "require('${tool_dir}/node_modules/fallow-type-aware/package.json').version" 2>/dev/null | tr -d '\r\n' || true)"
  if [ "$sidecar_installed_version" != "$sidecar_version" ]; then
    echo "::error::Installed fallow-type-aware ${sidecar_installed_version:-unknown}, expected ${sidecar_version}. The exact-version-match contract does not hold; aborting."
    exit 1
  fi

  if [ -n "${GITHUB_ENV:-}" ]; then
    printf 'FALLOW_TYPE_AWARE_BIN=%s\n' "$sidecar_bin" >> "$GITHUB_ENV"
    # Marks the wiring as Action-provisioned so `type-aware status` reports
    # github-action instead of environment-override.
    printf 'FALLOW_TYPE_AWARE_BIN_SOURCE=github-action\n' >> "$GITHUB_ENV"
  fi
  echo "Installed fallow-type-aware ${sidecar_version} sidecar at ${sidecar_bin}"
fi
