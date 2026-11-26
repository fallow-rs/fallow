# `cloud`: Log In to Fallow Cloud

Requires fallow 3.38.0 or later. Connect this machine to Fallow Cloud without copying a key. `login` runs the device flow: it prints a code and a URL, and after a user approves the code in the dashboard the cloud creates a read-only API key (`fallow_read_k1_`) for that user, organization and machine. The CLI stores the key and never prints it. Agents must not run `login`; only the user may approve it.

```bash
fallow cloud login
fallow cloud login --machine-name build-box
fallow cloud logout
```

| Flag | Description |
|------|-------------|
| `--machine-name <NAME>` | Name of the machine on the key (`login` only). Default: the host name |
| `--api-url <URL>` | Fallow Cloud API base URL. Default: `$FALLOW_API_URL`, else `https://api.fallow.cloud` |

Storage: the OS keychain (service `fallow-cloud`, account = the API base URL; `security` on macOS, `secret-tool` on Linux), else `~/.config/fallow/credentials` (mode 0600). `FALLOW_CREDENTIAL_STORE=file` skips the keychain; `FALLOW_CREDENTIAL_STORE=none` turns stored keys off. `logout` removes the stored key; the key stays valid until you revoke it in Settings.

Every command that reads a cloud key uses one order: `--api-key` > `$FALLOW_API_KEY` > the keychain > the credentials file.
