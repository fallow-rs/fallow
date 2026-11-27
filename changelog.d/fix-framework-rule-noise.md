### Fixed

- **`unused-component-emit` no longer reports an `update:<key>` event that a VueUse model ref emits.** `useVModel(props, key)` returns a ref whose setter emits `update:<key>` through the component instance, also without an `emit` argument. `useVModels(props)` returns one such ref for each prop. A write of the ref in the script (`ref.value = x`) or in the template (`v-model="ref"`, `ref = x`) now marks the event as used. A write to a nested field of the ref value does not run the setter, so the event stays unused. The extraction cache version changes, so the first run after the update parses each file again.
  (Closes [#5403](https://github.com/fallow-rs/fallow/issues/5403))
- **`unused-component-input` no longer reports an Angular input that a `@HostBinding` on the same member reads.** `@HostBinding("class.x") @Input() x` reflects the input onto the host element, so Angular reads it.
  (Closes [#5404](https://github.com/fallow-rs/fallow/issues/5404))
