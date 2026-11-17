### Changed

- **Bursts of merges to `main` start fewer CI runs.** The push runs of CI on `main` now share one concurrency group. A started run always finishes, and a newer push replaces a run that waits in the queue. The release commit keeps a group of its own, so the release gate always gets a result for it. The prose-only skip on `main` now also checks that the push run of the previous commit succeeded, because a replaced run leaves that commit without a result. This change affects contributors only.
