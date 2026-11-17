### Fixed

- **The GitHub Action no longer tells you to remove a suppression that only lacks a reason.** With `require-suppression-reason` on, the Action annotated a suppression comment without a reason as a stale suppression and said to remove it. It now shows the annotation "Suppression without a reason" and says to add the reason after `--`. The pull request summary names the missing reason in the same way.
