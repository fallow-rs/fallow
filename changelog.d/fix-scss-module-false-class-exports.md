### Fixed

- **SCSS Modules no longer report Sass module members as unused classes.**
  A namespaced function call such as `math.div(100px, 10)` or `utils.bar()`,
  and a namespaced mixin such as `@include utils.baz`, gave a false
  `unused-export` for `div`, `bar` and `baz`. The class scan now skips a
  `.member` that follows a namespace and comes before `(`, and the mixin name
  after `@include`. In `.module.sass` files, the scan also skips the mixin
  name in the `+utils.baz` include shorthand. A compound selector such as
  `button.primary` still
  exports `primary`. The extraction cache version changes, so the first run
  after the upgrade parses each file again.
  (Closes [#3839](https://github.com/fallow-rs/fallow/issues/3839))

- **CSS Modules no longer report view transition class names as unused
  classes.** In `::view-transition-new(.demo)` and the other
  `::view-transition-*()` pseudo-elements, `.demo` is a view transition class
  and not an element class. The SCSS class scan exported it, and fallow then
  gave a false `unused-export` for `demo`. The class scan now skips the
  argument of these pseudo-elements. The extraction cache version changes, so
  the first run after the upgrade parses each file again.
  (Closes [#3840](https://github.com/fallow-rs/fallow/issues/3840))
