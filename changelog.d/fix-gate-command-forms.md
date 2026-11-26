### Fixed

- **The commit gate of `fallow agent install` and `fallow hooks install` now audits more forms of a git commit or push.** The gate skipped `GIT commit`, `/usr/bin/git push`, `git {commit,status}`, `$G commit` and `g?t push`. It now reads `git` in any letter case and as the last part of a path. A command with an expansion, a brace, a glob or a backquote that also holds a `commit` or `push` word now goes to the audit. A command without such a word still skips the audit, so `ls -la` and `echo $HOME` stay as fast as before.
