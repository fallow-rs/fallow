### Fixed

- **`fallow dead-code` credits a member on a local that holds a method call result.** In `const rotated = local.rotate(angle); rotated.translate(1, 1);`, where `rotate(angle: number): Point`, fallow now credits `Point.translate`, as it does for the chain `local.rotate(angle).translate(1, 1)`. The receiver can be a typed parameter or a `new Point(...)` local, and the call can be optional (`local?.rotate(angle)`). A conditional init such as `const rotated = angle ? local.rotate(angle) : local` credits the member on each branch. The extraction cache version is 443. Thanks @Smrtnyk for the report.
  (Closes [#4552](https://github.com/fallow-rs/fallow/issues/4552))
