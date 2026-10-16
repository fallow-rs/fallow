### Fixed

- **`fallow mutation` no longer breaks Vue components that call `defineExpose`.**
  The `removed-call` operator wrapped the `defineExpose()` statement of a
  `<script setup>` block in a mutant switch. The Vue compiler then did not
  find the macro, the component threw `defineExpose is not defined`, and each
  test of the component failed in the dry run. A Vue macro call and its
  arguments now give no mutant.
