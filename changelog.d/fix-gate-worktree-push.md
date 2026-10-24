### Fixed

- **The agent gate audits only the worktree for a push that cannot send the
  session branch.** A strict `git push` from a linked worktree of the session
  repository also audited the session tree, because linked worktrees share
  refs. Untracked build output in the session thus blocked every push from a
  clean worktree. The gate now audits only the worktree when git shows that
  the push cannot send the session branch: the options are only `-u`, `-q`,
  `--set-upstream`, `--force-with-lease` and `--force-if-includes`, each
  refspec source is the checked-out branch of the worktree and not the
  session branch, and a push without a refspec has `push.default` unset or
  `simple`, `current`, `upstream` or `nothing` and no remote `push` or
  `mirror` setting. Any other push from a linked worktree, and any git error
  during the check, keeps the session audit. The strict push form now also
  accepts `--force-if-includes`. Run `fallow agent install` again to update an
  installed gate.
