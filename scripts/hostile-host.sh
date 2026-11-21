#!/usr/bin/env bash
# Run a command with a hostile home folder.
#
# Usage: scripts/hostile-host.sh [--dir <folder>] -- <command> [args...]
#
# The script makes a home folder that holds the files that a developer machine
# can have and a clean CI runner does not have:
#
# - `~/.fallow/bin/fallow-cov` with a `.sig` file. The binary fails.
# - `~/.gitconfig` with a `core.hooksPath` whose `pre-commit` hook fails.
# - a folder at the end of `PATH` with `fallow-cov` and `git` shims that fail.
#
# Then it runs the command with `HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME` and
# `XDG_CACHE_HOME` in that folder. A unit test that reads the host paths instead of its own
# `HostPaths` value then finds these files and fails. `CARGO_HOME` and
# `RUSTUP_HOME` keep their values, so that cargo finds its toolchain. The
# shim folder is the last `PATH` entry, so an installed `git` still wins.
#
# Without `--dir`, the script makes a folder in `$HOME/.cache` and removes it
# when the command ends. The folder must not be inside the system temp folder:
# the write guard allows a write there, and the write-scope tests need a home
# folder that is outside every allowed folder. The exit status is the status of
# the command.
set -euo pipefail

dir=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --dir)
      dir="$2"
      shift 2
      ;;
    --)
      shift
      break
      ;;
    *)
      echo "hostile-host: unknown argument: $1" >&2
      exit 2
      ;;
  esac
done
if [ "$#" -eq 0 ]; then
  echo "usage: scripts/hostile-host.sh [--dir <folder>] -- <command> [args...]" >&2
  exit 2
fi

if [ -z "$dir" ]; then
  mkdir -p "$HOME/.cache"
  dir="$(mktemp -d "$HOME/.cache/fallow-hostile-host.XXXXXX")"
  trap 'rm -rf "$dir"' EXIT
fi
mkdir -p "$dir"
dir="$(cd "$dir" && pwd -P)"

home="$dir/home"
shims="$dir/shims"
hooks="$dir/git-hooks"
mkdir -p "$home/.fallow/bin" "$home/.config" "$home/.local/share" "$home/.cache" "$shims" "$hooks"

write_failing() {
  local file="$1" name="$2"
  printf '#!/bin/sh\necho "hostile host: the test ran %s from %s" >&2\nexit 97\n' "$name" "$file" >"$file"
  chmod 755 "$file"
}

write_failing "$home/.fallow/bin/fallow-cov" "fallow-cov"
printf 'hostile-host-signature\n' >"$home/.fallow/bin/fallow-cov.sig"
write_failing "$shims/fallow-cov" "fallow-cov"
write_failing "$shims/git" "git"
write_failing "$hooks/pre-commit" "a pre-commit hook of the global Git config"
printf '[core]\n\thooksPath = %s\n' "$hooks" >"$home/.gitconfig"

export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
export HOME="$home"
export XDG_CONFIG_HOME="$home/.config"
export XDG_DATA_HOME="$home/.local/share"
export XDG_CACHE_HOME="$home/.cache"
export PATH="$PATH:$shims"

status=0
"$@" || status=$?
exit "$status"
