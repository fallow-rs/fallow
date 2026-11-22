### Fixed

- **On Windows, JSON output keeps the brackets of a route path.** A finding in `app/blog/[id]/page.tsx` showed as `app/blog/%5Bid%5D/page.tsx` in `--format json`, while SARIF and the other platforms gave the plain path. JSON paths now use forward slashes and keep `[` and `]` on each platform. SARIF and CodeClimate URIs still encode the brackets.
- **On Windows, `fallow mutation` replaces a stale run directory while another process reads a file in it.** Windows refuses to move a directory with an open file, so the run failed with "The directory is not empty. (os error 145)". The run now tries again for up to one second.
