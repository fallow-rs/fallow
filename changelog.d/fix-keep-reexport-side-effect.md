### Changed

- **`fallow fix` keeps the module load when it removes the last name of a re-export.** `export { Widget } from './widget'` loads and runs `./widget`, for example to register a custom element. Before, `fallow fix` deleted the full statement, so the module no longer ran. Now the fix replaces the statement with `import './widget';`. The fix still deletes the statement when another line of the file loads the same module, and for a type-only re-export, which loads nothing. Remove the bare import yourself when the module has no side effects.
