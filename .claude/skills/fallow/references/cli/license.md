# `license`: Manage the Fallow Cloud License

Manage the local JWT used to unlock continuous/cloud runtime monitoring. Single-capture local runtime analysis does not require a license. Verification is fully offline against an Ed25519 public key compiled into the binary. Only `--trial` and `refresh` hit the network (`api.fallow.cloud`, 5s connect / 10s total timeout).

```bash
fallow license activate --trial --email you@company.com
fallow license activate eyJhbGciOiJFZERTQSIsInR5cCI6IkpXVCJ9...
fallow license activate --from-file ./license.jwt
cat ./license.jwt | fallow license activate --stdin
fallow license status
fallow license refresh
fallow license deactivate
```

## Subcommands

| Subcommand | Purpose |
|------------|---------|
| `activate` | Install a JWT or start a 30-day trial. Pass a JWT as an argument, read it with `--from-file`, or pipe it to `fallow license activate --stdin`. |
| `status`   | Print tier, seats, features, days-until-expiry, and (when `refresh_after` has passed) a proactive refresh hint. |
| `refresh`  | Fetch a fresh JWT using the currently stored one as identity proof, falling back to a full-access API key when that token is missing or too stale. Exit 7 on network failure. |
| `deactivate` | Remove the local license file. |

## `activate` flags

| Flag | Type | Description |
|------|------|-------------|
| `--trial` | bool | Start a 30-day email-gated trial. Requires `--email`. **Rate-limited to 5 requests per hour per IP** - in CI or behind a shared NAT, start the trial locally and set `FALLOW_LICENSE` on the runner. |
| `--email <ADDR>` | string | Email for the trial flow. On success, `trialEndsAt` is printed to stdout so you can see the trial window without decoding the JWT. |
| `--from-file <PATH>` | path | Read a JWT from a file. |
| `--stdin` | bool | Read a JWT from stdin. Conflicts with `--from-file` and positional JWT. |

## `refresh` flags

| Flag | Type | Description |
|------|------|-------------|
| `--api-key <KEY>` | string | Full-access API key used as the bearer when the stored license JWT is missing or the cloud reports it as `token_stale`. Precedence: this flag > `$FALLOW_API_KEY`. Prefer the environment variable on shared runners so the key stays out of argv. |

## Credential order for `refresh`

1. The stored license JWT (`FALLOW_LICENSE`, `FALLOW_LICENSE_PATH`, or `~/.fallow/license.jwt`).
2. A full-access API key, when the stored JWT is absent or the cloud answers `token_stale`.

A machine that has not run fallow for weeks holds a JWT the cloud refuses, and the trial endpoint rejects an organisation that already pays, so the API key is the recovery path. With neither credential available, `refresh` names that route instead of the trial flow.

## Storage precedence

1. `FALLOW_LICENSE` (env var holding the full JWT string)
2. `FALLOW_LICENSE_PATH` (env var pointing at a file)
3. `~/.fallow/license.jwt` (default; written `chmod 0600` on Unix)

## Grace ladder

| Days past `exp` | State | Behavior |
|-----------------|-------|----------|
| `<= 7` | ExpiredWarning | Analysis runs; CLI prints a refresh hint |
| `> 7, <= 30` | ExpiredWatermark | Analysis runs; output gains a visible watermark until refreshed |
| `> 30` | HardFail | Continuous/cloud runtime monitoring is blocked; run `fallow license refresh` or start a new trial |

## Actionable error messages

On HTTP error from `api.fallow.cloud`, fallow parses the `{error, message, code}` envelope and maps known codes to targeted hints:

| Operation + code | CLI message |
|------------------|-------------|
| `refresh` + `token_stale` | your stored license is too stale to refresh: set `FALLOW_API_KEY` to a full-access key and run `fallow license refresh` again (generate one at `https://fallow.cloud/settings#api-keys`) |
| `refresh` + `invalid_token` | `your stored license token is missing required claims. Reactivate with: fallow license activate --trial --email <addr>` |
| `refresh` or `trial` + `unauthorized` | `authentication failed. Reactivate with: fallow license activate --trial --email <addr>` |
| `trial` + `rate_limit_exceeded` | `trial creation is rate-limited to 5 per hour per IP. Wait an hour or retry from a different network (in CI, start the trial locally and set FALLOW_LICENSE on the runner).` |

Unknown codes fall back to the backend's `message` field, or the raw body.

## Clock skew

License verification rejects JWTs whose `iat` claim is more than 24 hours in the future relative to the local system clock. The same check catches both a forward-signed JWT and a local clock behind reality. Rejection exits non-zero so paid features fail closed.

| Env var | Default | Effect |
|---------|---------|--------|
| `FALLOW_LICENSE_SKEW_TOLERANCE_SECONDS` | `86400` (24h) | Overrides the tolerance window applied to the `iat` claim. Lenient parsing: unset / empty / unparsable / negative all fall back to the default. |

Common non-user causes: CI containers without NTP, machines with a dead BIOS battery, drifted laptop clocks after long sleep.

## Exit Codes

| Code | Meaning |
|------|---------|
| `0`  | Valid license (or trial/refresh succeeded) |
| `2`  | Bad invocation (missing email for `--trial`, unreadable file) |
| `3`  | License missing, hard-fail expired, malformed JWT, or clock skew exceeds tolerance |
| `7`  | Network failure or non-success HTTP status from `api.fallow.cloud` |
