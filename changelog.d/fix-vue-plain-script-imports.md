### Fixed

- **`fallow dead-code` no longer reports an export as unused when a Vue `<script setup>` block or the template uses an import of the plain `<script>` block.** Vue compiles both blocks into one module, so fallow now credits a name that one block imports and another block or the template uses. A Svelte component gets the same fix: the instance script and the markup can use an import of the module script (`<script module>` or `<script context="module">`). An import that no block uses stays unused. The extraction cache version changes, so the first run after the update parses all files again.
  (Closes [#4672](https://github.com/fallow-rs/fallow/issues/4672))
