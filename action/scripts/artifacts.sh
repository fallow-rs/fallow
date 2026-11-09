#!/usr/bin/env bash

# shellcheck source=../../ci/scripts/mutation_common.sh
source "$(dirname -- "${BASH_SOURCE[0]}")/../../ci/scripts/mutation_common.sh"

prepare_artifacts() {
  local dir=$1 name
  shift
  fallow_artifact_check "$dir" directory || return 2
  for name in "$@"; do
    fallow_artifact_check "$dir/$name" || return 2
  done
  if ! mkdir -p -- "$dir"; then
    printf '%s\n' '::error::Failed to create the artifact directory.' >&2
    return 2
  fi
}
