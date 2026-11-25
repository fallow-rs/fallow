### Performance

- **Plugin `used_exports` rules no longer slow down monorepos with many workspaces.** Each workspace gets its own copy of the rules of a plugin such as Next.js, with the workspace path as prefix. Fallow now groups the rules by the literal directory of each pattern. A module is tested only against the rules of its own directory chain, not against the rules of every workspace. The unused-export analysis, `--trace`, `--performance` and `audit` use the same index. On a generated monorepo with 75 Next.js workspaces and about 200,000 files, the CPU time of `fallow dead-code` fell by about 40%, and by about half with `--performance`. The output does not change.
  (Closes [#5196](https://github.com/fallow-rs/fallow/issues/5196))
