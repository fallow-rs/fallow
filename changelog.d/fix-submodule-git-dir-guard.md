### Fixed

- **A write through a symlink into the Git directory of a superproject is now refused.** When the project root is in a submodule, the write guard protected only the Git directory of the submodule. A superproject can keep its Git directory outside its work tree (`git init --separate-git-dir`), for example in a temp directory. A report path such as `--output-file /tmp/r.json` that is a link into that directory now fails, the same as a link into `.git`. A `.git` file that fallow cannot read, or that names no Git directory, now makes each link target count as protected.
