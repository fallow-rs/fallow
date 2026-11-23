### Security

- **The `cookie` dependency is at 0.18.3 for RUSTSEC-2026-0594.** Version 0.18.2 can repeat the nonces of private cookies in a process that forks. The release binaries now build with the fixed version.
