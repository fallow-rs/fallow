### Fixed

- **`fallow agent install` now refreshes a stale fallow skill under `.agents/skills/` when it detects only Claude Code.** Before, a `.claude` folder made the install write only `.claude/skills/`, so `fallow agent status` kept the `.agents` row `stale` after the suggested command. The install now updates each `.agents` skill that has the fallow marker, in user scope and in project scope. It does not create an absent `.agents` skill, and it does not change a skill without the marker.
  (Closes [#5132](https://github.com/fallow-rs/fallow/issues/5132))
