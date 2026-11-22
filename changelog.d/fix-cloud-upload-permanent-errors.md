### Fixed

- **`fallow coverage upload-inventory` and `upload-static-findings` fail on a permanent HTTP error, also with `--ignore-upload-errors`.**
  A 404, 409, 410 or 422 response went to the transient server error class, so `--ignore-upload-errors` changed a wrong project ID or endpoint into a warning with exit 0. These statuses now exit with the validation code. A 5xx, 408 or 429 response stays transient. A 413 response to `upload-inventory` now exits with the payload-too-large code, as it does for `upload-static-findings`.
- **A git-SHA keyed upload no longer treats a failed `git status` as a clean checkout.**
  When `git status` did not start or failed, the upload continued as if the working tree was clean. Without `--git-sha`, the upload now stops with a validation error. With `--git-sha`, for example in a build without a git checkout, the upload continues and prints a warning. `--allow-dirty` also lets the upload continue.
