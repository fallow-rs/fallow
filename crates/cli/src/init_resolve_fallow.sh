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
FALLOW_FIX="update the fallow devDependency of the project"
if [ -x ./node_modules/.bin/fallow ] && fallow_runs ./node_modules/.bin/fallow ./node_modules/.bin/fallow; then
  run_fallow() { ./node_modules/.bin/fallow "$@"; }
elif command -v fallow >/dev/null 2>&1 && fallow_runs "$(command -v fallow)" fallow; then
  FALLOW_FIX="upgrade the fallow on PATH"
  run_fallow() { fallow "$@"; }
elif command -v yarn >/dev/null 2>&1 \
  && FALLOW_BIN="$(yarn bin fallow 2>/dev/null)" \
  && [ -n "$FALLOW_BIN" ] \
  && fallow_runs "yarn exec fallow" yarn exec fallow --; then
  run_fallow() { yarn exec fallow -- "$@"; }
elif command -v npx >/dev/null 2>&1 \
  && FALLOW_VERSION="$(npx --no-install fallow --version 2>/dev/null)" \
  && case "$FALLOW_VERSION" in fallow*) true ;; *) false ;; esac; then
  FALLOW_DESC="npx --no-install fallow"
  run_fallow() { npx --no-install fallow "$@"; }
elif [ -n "$FALLOW_BROKEN" ]; then
  echo "fallow-hook: blocked: $FALLOW_BROKEN cannot print its fallow version (${FALLOW_BROKEN_ERR:-no error output})." >&2
  echo "fallow-hook: reinstall fallow, or skip the audit for one commit with git commit --no-verify." >&2
  exit 1
else
  echo "fallow-hook: fallow not found (tried node_modules/.bin, PATH, yarn and npx --no-install), skipping the pre-commit audit." >&2
  exit 0
fi
# The `--staged` flag of `fallow audit` exists since fallow 3.43.0. An older
# fallow stops with a parser error, so the hook names the runner and the fix
# instead. A version that does not parse as numbers skips this check.
# The npm launcher prints a `verified:` line after the version line, so the
# version ends at the first space or newline.
FALLOW_NUM="${FALLOW_VERSION#fallow }"
FALLOW_NUM="${FALLOW_NUM%%[[:space:]]*}"
FALLOW_MAJOR="${FALLOW_NUM%%.*}"
FALLOW_MINOR="${FALLOW_NUM#*.}"
FALLOW_MINOR="${FALLOW_MINOR%%.*}"
case "$FALLOW_MAJOR.$FALLOW_MINOR" in
  *[!0-9.]* | .* | *.) ;;
  *)
    if [ "$FALLOW_MAJOR" -lt 3 ] || { [ "$FALLOW_MAJOR" -eq 3 ] && [ "$FALLOW_MINOR" -lt 43 ]; }; then
      echo "fallow-hook: blocked: $FALLOW_DESC is fallow $FALLOW_NUM, below the required 3.43.0." >&2
      echo "fallow-hook: $FALLOW_FIX, or skip the audit for one commit with git commit --no-verify." >&2
      exit 1
    fi
    ;;
esac
