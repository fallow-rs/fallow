#!/usr/bin/env bash
# Self-test for scripts/hostile-host.sh. It checks that the command sees the
# hostile home folder, the failing shims and the global Git hook, that cargo
# keeps its own folders, that the exit status of the command comes back, and
# that the script removes the folder that it made.
# Run: bash scripts/test-hostile-host.sh
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HOSTILE="$SCRIPT_DIR/hostile-host.sh"

pass=0
fail=0
check() { # description, expected, actual
  if [ "$2" = "$3" ]; then pass=$((pass + 1)); else fail=$((fail + 1)); echo "FAIL: $1 (expected $2, got $3)"; fi
}

bash "$HOSTILE" >/dev/null 2>&1
check "no command" 2 $?
bash "$HOSTILE" --unknown -- true >/dev/null 2>&1
check "unknown argument" 2 $?

real_home="$HOME"
out="$(env -u CARGO_HOME -u RUSTUP_HOME bash "$HOSTILE" -- sh -c '
  echo "home=$HOME"
  echo "cargo_home=$CARGO_HOME"
  echo "rustup_home=$RUSTUP_HOME"
  echo "xdg_config=$XDG_CONFIG_HOME"
  echo "xdg_data=$XDG_DATA_HOME"
  test -x "$HOME/.fallow/bin/fallow-cov" && echo "fake-cov=yes"
  test -f "$HOME/.fallow/bin/fallow-cov.sig" && echo "fake-sig=yes"
  "$HOME/.fallow/bin/fallow-cov" >/dev/null 2>&1; echo "fake-cov-status=$?"
  hooks="$(git config --global core.hooksPath)"
  "$hooks/pre-commit" >/dev/null 2>&1; echo "hook-status=$?"
  last="${PATH##*:}"
  "$last/git" >/dev/null 2>&1; echo "git-shim-status=$?"
  "$last/fallow-cov" >/dev/null 2>&1; echo "cov-shim-status=$?"
')"
line() { printf '%s\n' "$out" | grep "^$1=" | head -1 | cut -d= -f2-; }
home="$(line home)"
check "home is replaced" 1 "$([ -n "$home" ] && [ "$home" != "$real_home" ] && echo 1 || echo 0)"
check "home is outside the temp folder" 1 "$(case "$home" in "$real_home"/.cache/*) echo 1 ;; *) echo 0 ;; esac)"
check "unset cargo home is the real default" "$real_home/.cargo" "$(line cargo_home)"
check "unset rustup home is the real default" "$real_home/.rustup" "$(line rustup_home)"
check "xdg config is in the home" "$home/.config" "$(line xdg_config)"
check "xdg data is in the home" "$home/.local/share" "$(line xdg_data)"
check "fake add-on" yes "$(line fake-cov)"
check "fake signature" yes "$(line fake-sig)"
check "fake add-on fails" 97 "$(line fake-cov-status)"
check "global hook fails" 97 "$(line hook-status)"
check "git shim fails" 97 "$(line git-shim-status)"
check "add-on shim fails" 97 "$(line cov-shim-status)"
check "folder is removed" 1 "$([ -n "$home" ] && [ ! -e "$home" ] && echo 1 || echo 0)"

given_cargo="$(CARGO_HOME=/opt/given-cargo RUSTUP_HOME=/opt/given-rustup bash "$HOSTILE" -- sh -c 'echo "$CARGO_HOME $RUSTUP_HOME"')"
check "a set cargo and rustup home stay" "/opt/given-cargo /opt/given-rustup" "$given_cargo"

if command -v cargo >/dev/null 2>&1 && [ -x "${CARGO_HOME:-$real_home/.cargo}/bin/cargo" ]; then
  bash "$HOSTILE" -- "${CARGO_HOME:-$real_home/.cargo}/bin/cargo" --version >/dev/null 2>&1
  check "cargo runs under the wrapper" 0 $?
fi

bash "$HOSTILE" -- sh -c 'exit 7'
check "exit status of the command" 7 $?

dir="$(mktemp -d)"
trap 'rm -rf "$dir"' EXIT
bash "$HOSTILE" --dir "$dir/given" -- true
check "a given folder stays" 1 "$([ -x "$dir/given/home/.fallow/bin/fallow-cov" ] && echo 1 || echo 0)"

echo "hostile-host self-test: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
