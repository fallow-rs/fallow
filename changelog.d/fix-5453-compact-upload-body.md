### Fixed

- **`fallow coverage upload-inventory` sends a compact JSON body.** The CLI sent each JSON request body as indented JSON. On a large repository, the indentation pushed the inventory body above the 25 MiB request cap of the Fallow Cloud API. The upload then failed with HTTP 413. The CLI now sends compact JSON, which makes an inventory body about a quarter smaller. The same change applies to these requests:
  - `upload-static-findings` and `upload-source-maps`
  - `coverage analyze --cloud`
  - the login and trial requests, and telemetry
  - the review comments that fallow posts to GitHub and GitLab

  (Closes [#5453](https://github.com/fallow-rs/fallow/issues/5453))
