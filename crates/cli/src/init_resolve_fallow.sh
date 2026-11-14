FALLOW_BROKEN=""
FALLOW_BROKEN_ERR=""
fallow_runs() {
  FALLOW_DESC="$1"
  shift
  FALLOW_VERSION="$("$@" --version 2>/dev/null)" \
    && case "$FALLOW_VERSION" in fallow*) true ;; *) false ;; esac \
    && return 0
  if [ -z "$FALLOW_BROKEN" ]; then
    FALLOW_BROKEN="$FALLOW_DESC"
    FALLOW_BROKEN_ERR="$("$@" --version 2>&1 >/dev/null | { IFS= read -r line; printf '%s' "$line"; })"
  fi
  return 1
}
if command -v fallow >/dev/null 2>&1 && fallow_runs "$(command -v fallow)" fallow; then
  run_fallow() { fallow "$@"; }
elif [ -x ./node_modules/.bin/fallow ] && fallow_runs ./node_modules/.bin/fallow ./node_modules/.bin/fallow; then
  run_fallow() { ./node_modules/.bin/fallow "$@"; }
elif command -v yarn >/dev/null 2>&1 \
  && FALLOW_BIN="$(yarn bin fallow 2>/dev/null)" \
  && [ -n "$FALLOW_BIN" ] \
  && fallow_runs "yarn exec fallow" yarn exec fallow --; then
  run_fallow() { yarn exec fallow -- "$@"; }
elif command -v npx >/dev/null 2>&1 \
  && FALLOW_VERSION="$(npx --no-install fallow --version 2>/dev/null)" \
  && case "$FALLOW_VERSION" in fallow*) true ;; *) false ;; esac; then
  run_fallow() { npx --no-install fallow "$@"; }
elif [ -n "$FALLOW_BROKEN" ]; then
  echo "fallow-hook: blocked: $FALLOW_BROKEN cannot print its fallow version (${FALLOW_BROKEN_ERR:-no error output})." >&2
  echo "fallow-hook: reinstall fallow, or skip the audit for one commit with git commit --no-verify." >&2
  exit 1
else
  echo "fallow-hook: fallow not found (tried PATH, node_modules/.bin, yarn and npx --no-install), skipping the pre-commit audit." >&2
  exit 0
fi
