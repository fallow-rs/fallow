### Fixed

- **`fallow health --hotspots` gives the same churn from the cache as from a `--no-cache` run.** Two project roots that share one `FALLOW_CACHE_DIR` no longer read the churn of each other, and a run with an earlier HEAD date no longer reuses an entry that lacks the older days. The window cutoff applies to each commit, so a commit in the window behind an older-dated commit counts in a cold run too. The churn cache moves to version 10 and is rebuilt once.
