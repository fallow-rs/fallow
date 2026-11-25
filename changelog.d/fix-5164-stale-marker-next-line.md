### Fixed

- **A stale `fallow-ignore-next-line` marker now names the cause on the next line.** When the next line has a finding of another kind, the message names that kind and the marker that suppresses it, for example `use fallow-ignore-next-line unused-type` above a type re-export. When the next line is blank, the message says so: the marker applies only to the line directly after it. JSON output gives the same facts in the new `next_line_kinds` and `next_line_blank` fields.
  (Closes [#5164](https://github.com/fallow-rs/fallow/issues/5164))
- **The docs of `duplicates.threshold` and `--threshold` now say that `0` turns the gate off.** The value `0` is not zero tolerance. To fail on any clone group, use `--fail-on-issues`.
  (Closes [#5165](https://github.com/fallow-rs/fallow/issues/5165))
