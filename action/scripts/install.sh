#!/usr/bin/env bash
set -euo pipefail

# shellcheck source=workflow-command.sh
source "$(dirname -- "${BASH_SOURCE[0]}")/workflow-command.sh"

# Install fallow binary via npm, plus the exact-version fallow-type-aware
# sidecar when the project or the action opts into type-aware analysis.
# Optional env: FALLOW_VERSION, INPUT_ROOT, INPUT_CONFIG, INPUT_TYPE_AWARE,
# FALLOW_INSTALL_DRY_RUN, GITHUB_ENV, GITHUB_ACTION_PATH, GITHUB_ACTION_REF,
# RUNNER_TEMP.

# Each optional variable gets its default here, so `set -u` fails on a typo in
# a name instead of on an input that the workflow did not set.
: "${FALLOW_VERSION:=}" "${INPUT_ROOT:=.}" "${INPUT_CONFIG:=}"
: "${INPUT_TYPE_AWARE:=}" "${FALLOW_INSTALL_DRY_RUN:=}" "${GITHUB_ENV:=}"
: "${GITHUB_ACTION_PATH:=}" "${GITHUB_ACTION_REF:=}" "${RUNNER_TEMP:=}"

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

# Print the fallow entry of the catalog that a `catalog:` or `catalog:<name>`
# spec refers to. The script walks up from the action root to the first
# directory with a catalog file: a pnpm-workspace.yaml (pnpm), or a
# package.json with `catalog`, `catalogs` or `workspaces.catalog(s)` (Bun).
# It prints nothing when the file, the catalog or the entry is missing. The
# YAML parser reads only block mappings, which is the form that pnpm writes
# and documents.
catalog_fallow_spec() {
  node - "$1" "$2" <<'NODE'
const fs = require("node:fs");
const path = require("node:path");
const [start, catalog] = process.argv.slice(2);
const unquote = (text) => text.trim().replace(/^(["'])(.*)\1$/, "$2");
const stripComment = (text) => text.replace(/\s+#.*$/, "");
// The default catalog is `catalog` or `catalogs.default`. A named catalog is
// `catalogs.<name>`.
const tables = catalog === "default" ? [["catalog"], ["catalogs", "default"]] : [["catalogs", catalog]];

const fromYaml = (file) => {
  const lines = fs.readFileSync(file, "utf8").split(/\r?\n/);
  for (const table of tables) {
    const want = [...table, "fallow"];
    const stack = [];
    for (const raw of lines) {
      const line = stripComment(raw);
      if (!line.trim() || line.trim().startsWith("#")) continue;
      const match = /^(\s*)(?:"([^"]+)"|'([^']+)'|([^\s:][^:]*?))\s*:(?:\s+(.*))?$/.exec(line);
      if (!match) continue;
      const indent = match[1].length;
      const key = match[2] ?? match[3] ?? match[4];
      while (stack.length && stack[stack.length - 1].indent >= indent) stack.pop();
      stack.push({ indent, key });
      const value = match[5] === undefined ? "" : unquote(match[5]);
      if (value && stack.length === want.length && stack.every((entry, index) => entry.key === want[index])) {
        return value;
      }
    }
  }
  return "";
};

// Bun reads a catalog from the root package.json, at the top level or under
// an object-form `workspaces`.
const bunCatalogHolders = (pkg) =>
  [pkg, pkg?.workspaces].filter(
    (holder) => holder && typeof holder === "object" && !Array.isArray(holder) && (holder.catalog || holder.catalogs),
  );

const fromPackageJson = (holders) => {
  for (const holder of holders) {
    for (const table of tables) {
      const entry = table.reduce((node, key) => node?.[key], holder)?.fallow;
      if (typeof entry === "string" && entry.trim()) return entry.trim();
    }
  }
  return "";
};

let dir = path.resolve(start);
for (;;) {
  const yaml = path.join(dir, "pnpm-workspace.yaml");
  if (fs.existsSync(yaml)) {
    console.log(fromYaml(yaml));
    process.exit(0);
  }
  const manifest = path.join(dir, "package.json");
  if (fs.existsSync(manifest)) {
    let holders = [];
    try {
      holders = bunCatalogHolders(JSON.parse(fs.readFileSync(manifest, "utf8")));
    } catch {
      holders = [];
    }
    if (holders.length) {
      console.log(fromPackageJson(holders));
      process.exit(0);
    }
  }
  const parent = path.dirname(dir);
  if (parent === dir) process.exit(0);
  dir = parent;
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
  echo "::notice::Using fallow version from action input: $(escape_command_data "$install_spec")"
elif [[ "$project_spec" =~ ^catalog:(.*)$ ]]; then
  # pnpm and Bun replace a catalog spec with the catalog entry, so the action
  # installs that entry. A move to latest skips the pin and the release-age gate.
  catalog_name="$(trim "${BASH_REMATCH[1]}")"
  catalog_name="${catalog_name:-default}"
  catalog_spec="$(catalog_fallow_spec "$root" "$catalog_name" 2>/dev/null || true)"
  catalog_spec="$(trim "$catalog_spec")"
  if [ -n "$catalog_spec" ] && is_safe_version_spec "$catalog_spec"; then
    install_spec="$catalog_spec"
    version_source="the fallow entry of the catalog '${catalog_name}'"
    echo "::notice::Using fallow version from $(escape_command_data "catalog '${catalog_name}': ${install_spec}")"
  else
    echo "::warning::Cannot resolve the fallow package.json spec '$(escape_command_data "$project_spec")' from the catalog '$(escape_command_data "$catalog_name")' in pnpm-workspace.yaml or the workspace root package.json. Set the action 'version' input to pin fallow. The action installs the latest release."
    install_spec="latest"
    version_source="the latest published release"
  fi
elif [ -n "$project_spec" ]; then
  if is_safe_version_spec "$project_spec"; then
    install_spec="$project_spec"
    version_source="the fallow dependency in ${root}/package.json"
    echo "::notice::Using fallow version from $(escape_command_data "${root}/package.json: ${install_spec}")"
  else
    echo "::warning::Ignoring unsupported fallow package.json spec '$(escape_command_data "$project_spec")'. Use a semver version or range, or set the action 'version' input explicitly."
    install_spec="latest"
    version_source="the latest published release"
  fi
else
  install_spec="latest"
  version_source="the latest published release"
fi

if ! is_safe_version_spec "$install_spec"; then
  echo "::error::Invalid version specifier: $(escape_command_data "$install_spec"). Use 'latest' or a semver version/range like '2.52.2' or '^2.52.0'."
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
  # version_source can hold a catalog name from package.json, so escape it.
  echo "::error::Verification ran against fallow $(escape_command_data "$installed_fallow_version"), installed from $(escape_command_data "$version_source"). The Action ref ($(escape_command_data "${GITHUB_ACTION_REF:-see your workflow}")) selects the Action code, not the CLI version. Apply the recommended fix in the verification error above."
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
