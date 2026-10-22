### Fixed

- **The GitHub Action no longer fails `command: audit` on a default shallow checkout.** On a `pull_request` event, a depth-1 checkout of the merge commit has no merge base with the PR base, so `fallow audit --changed-since` exited 2 with "no merge base". The Action now fetches the smallest history that gives a merge base: the base commit, then the parents of the merge commit, then a bounded deepen. A failed fetch does not fail the job. `fetch-depth: 0` stays the recommended setup.
