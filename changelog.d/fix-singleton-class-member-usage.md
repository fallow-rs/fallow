### Fixed

- **`fallow dead-code` no longer reports the members of a singleton class that other code calls through an exported instance.** For `export const sm = SceneManager.getInstance()`, a call `sm.createScene()` in another module now counts as a use of `SceneManager.createScene`. This works when the class is in the same file or is imported, and when a local constant is exported with `export { sm }`. A static getter without a return type annotation also counts as a factory when it returns the instance that a static field holds: `return SceneManager.instance`, `return this.#instance!`, `return SceneManager.instance ?? (SceneManager.instance = new SceneManager())`, or `return (SceneManager.#instance ??= new SceneManager())`. A static method that returns another value, or the class itself (`return this`), still credits no instance member. When two different factory calls in one file give values to the same name, the export credits no class. The cache version changes, so the first run after the update parses every file again.
  (Closes [#5233](https://github.com/fallow-rs/fallow/issues/5233))

### Changed

- **The `usedClassMembers` warning for a scoped rule that matches nothing now names the scope.** A rule with `"extends": "SceneManager"` applies to the subclasses of `SceneManager`, not to `SceneManager` itself. The warning now says so, and it names the suppression for one class: `// fallow-ignore-file unused-class-member` in the file of that class. The schema text of `usedClassMembers` and `extends` gives the same information.
