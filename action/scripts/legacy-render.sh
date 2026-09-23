#!/usr/bin/env bash
# Shared helpers for the legacy jq renderers in action/jq/.
#
# summary.sh and annotate.sh source this file. The legacy renderers serve only
# fallow binaries before NATIVE_VERSION, which have no `fallow report`. They get
# no updates for new issue kinds, because those binaries do not emit them.
#
# Optional env: FALLOW_BIN

# The first release with `fallow report --format github-summary` and
# `--format github-annotations`.
NATIVE_VERSION="3.4.2"
NATIVE_MAJOR=3
NATIVE_MINOR=4
NATIVE_PATCH=2

# Print the version that `fallow --version` reports, or nothing when the
# output has no version number.
fallow_binary_version() {
  local raw
  raw=$("${FALLOW_BIN:-fallow}" --version 2>/dev/null | tr -d '\r\n') || raw=""
  printf '%s' "$raw" | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -n 1 || true
}

# True when the version is older than NATIVE_VERSION.
version_before_native() {
  local major minor patch
  IFS=. read -r major minor patch <<< "$1"
  (( major < NATIVE_MAJOR || (major == NATIVE_MAJOR && (minor < NATIVE_MINOR || (minor == NATIVE_MINOR && patch < NATIVE_PATCH))) ))
}

# Print a `::notice::` that says the legacy renderer ran for the surface in $1
# (for example "job summary"), and how to get the native renderer. The notice
# names the version only when it is known and older than NATIVE_VERSION, so it
# never states a wrong version.
legacy_renderer_notice() {
  local surface="$1" version
  version=$(fallow_binary_version)
  if [ -n "$version" ] && ! version_before_native "$version"; then
    echo "::notice::fallow ${version} has native GitHub rendering, but the probe 'fallow report --help' failed in the Analyze step. fallow renders the ${surface} with the legacy renderer, which gets no updates. Examine the Analyze step log."
    return 0
  fi
  local subject="This fallow version"
  [ -n "$version" ] && subject="fallow ${version}"
  echo "::notice::${subject} predates native GitHub rendering (${NATIVE_VERSION}). fallow renders the ${surface} with the legacy renderer, which gets no updates. Set the action 'version' input, or the fallow version in package.json, to ${NATIVE_VERSION} or later."
}
