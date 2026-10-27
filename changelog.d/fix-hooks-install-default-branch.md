### Security

- **`fallow hooks install` validates the detected default branch name.**
  Without `--branch`, the command reads the remote default branch name from
  `origin/HEAD` and writes it into the generated pre-commit hook. A crafted
  name could put shell code into the hook. The command now applies the same
  ref validator that `--branch` uses. When the name fails, the command prints
  a warning and uses `main` as the fallback. The existing-hook and lefthook
  hints and the install summary get the same validated name.
