#!/bin/sh
# Run the Linux credential store tests of `fallow cloud login`:
#   - without a D-Bus session, the key goes to a credentials file with mode 0600;
#   - with a D-Bus session and an unlocked gnome-keyring, the key goes to the
#     Secret Service through `secret-tool`.
# Both tests use a mock device flow, so they need no Fallow Cloud account.
#
# Usage:
#   sh scripts/linux-keychain-test.sh           On a Linux host with dbus,
#                                               gnome-keyring and secret-tool.
#   sh scripts/linux-keychain-test.sh --docker  In a Linux container, from any
#                                               host with Docker.
#
# The keyring lives in a temporary directory that the script removes at exit,
# so the script does not touch the keyring of the user.
set -eu

repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo"

FALLBACK_TEST=cloud_login_linux_keychain_tests::cloud_login_without_a_dbus_session_stores_the_key_in_a_0600_file
SECRET_SERVICE_TEST=cloud_login_linux_keychain_tests::cloud_login_with_a_secret_service_stores_the_key_in_the_keyring
KEYCHAIN_ROUND_TRIP_TEST=coverage::credentials::tests::keychain_round_trip
# The password of the temporary keyring. It protects no real secret.
KEYRING_PASSWORD=fallow-linux-keychain-test

if [ "${1:-}" = "--docker" ]; then
  toolchain=$(awk -F '"' '/^[[:space:]]*channel[[:space:]]*=/ {print $2; exit}' rust-toolchain.toml)
  # Named volumes keep the Linux build and the registry apart from the host
  # target directory and make a second run incremental.
  exec docker run --rm \
    -v "$repo:/src" -w /src \
    -v fallow-linux-keychain-target:/target -e CARGO_TARGET_DIR=/target \
    -v fallow-linux-keychain-registry:/usr/local/cargo/registry \
    "rust:${toolchain}" \
    sh -c 'apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends dbus gnome-keyring libsecret-tools >/dev/null && sh scripts/linux-keychain-test.sh'
fi

# `cargo test --exact` exits 0 when no test has the name. A renamed test then
# runs zero tests and the check stays green. Each call must pass exactly one
# test, else the script fails.
run_one_test() {
  log=$(mktemp)
  status=0
  cargo test --locked -p fallow-cli "$@" >"$log" 2>&1 || status=$?
  cat "$log"
  passed=$(grep -c '^test result: ok\. 1 passed;' "$log" || true)
  rm -f "$log"
  if [ "$status" -ne 0 ]; then
    exit "$status"
  fi
  if [ "$passed" -ne 1 ]; then
    echo "linux-keychain-test: no test ran for: $*. Check the test name in this script." >&2
    exit 1
  fi
}

# The tests that need the Secret Service. The script runs this part again
# inside the D-Bus session that it starts below.
if [ "${1:-}" = "--in-session" ]; then
  printf "%s" "$KEYRING_PASSWORD" | gnome-keyring-daemon --unlock --components=secrets >/dev/null
  run_one_test --test integration -- --ignored --exact "$SECRET_SERVICE_TEST"
  run_one_test --lib -- --ignored --exact "$KEYCHAIN_ROUND_TRIP_TEST"
  exit 0
fi

if [ "$(uname -s)" != Linux ]; then
  echo "linux-keychain-test: this host is not Linux. Run: sh scripts/linux-keychain-test.sh --docker" >&2
  exit 2
fi
for tool in dbus-run-session gnome-keyring-daemon secret-tool; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "linux-keychain-test: $tool is missing. Install dbus, gnome-keyring and libsecret-tools." >&2
    exit 2
  fi
done

cargo build --locked -p fallow-cli --bin fallow --test integration

echo "linux-keychain-test: credentials file without a D-Bus session"
(
  unset DBUS_SESSION_BUS_ADDRESS
  run_one_test --test integration -- --exact "$FALLBACK_TEST"
)

echo "linux-keychain-test: Secret Service in a D-Bus session"
keyring_home=$(mktemp -d)
trap 'rm -rf "$keyring_home"' EXIT INT TERM
mkdir -p "$keyring_home/data/keyrings" "$keyring_home/runtime"
chmod 700 "$keyring_home/runtime"
# gnome-keyring creates and unlocks the `login` keyring with the password on
# stdin. Without the keyrings directory, it cannot create the keyring, and
# `secret-tool` fails with "Object does not exist".
XDG_DATA_HOME="$keyring_home/data" \
  XDG_RUNTIME_DIR="$keyring_home/runtime" \
  dbus-run-session -- sh -eu scripts/linux-keychain-test.sh --in-session
