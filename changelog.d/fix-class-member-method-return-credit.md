### Fixed

- **`fallow dead-code` credits a member on the return value of a method that a base class declares.** In `class Viewer extends Floor`, code such as `const table = this.find(); table?.setVip();` now credits `setVip`, where `Floor` declares `find(): Table | undefined` in another file. The direct form `this.find()?.setVip()` is also credited. When the return type is a union alias such as `type Table = RectTable | RoundTable`, each class of the union gets the credit. Thanks @Smrtnyk for the report.
  (Closes [#4183](https://github.com/fallow-rs/fallow/issues/4183))
- **`fallow dead-code` credits a member on the return value of a method call with arguments.** A chain such as `local.rotate(1).translate(1, 1)`, where `rotate(angle: number): Point`, now credits `Point.translate`, as `local.rotate().translate()` already did. A chain with arguments at more calls, such as `rotate(1).translate(2, 2).scale(3)`, also gives credit. A plain function call with arguments, such as `make(1).paint()`, gives no credit. The extraction cache version is 407. Thanks @Smrtnyk for the report.
  (Closes [#4184](https://github.com/fallow-rs/fallow/issues/4184))
