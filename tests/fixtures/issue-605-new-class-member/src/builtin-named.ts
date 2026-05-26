// A user-defined class whose name collides with a global builtin
// (`is_builtin_constructor` would match "URL"). The extraction layer must NOT
// drop `new URL().parse()` on the basis of the name: the analyze layer resolves
// `URL` to THIS user export and credits `parse`. Caught by Codex's parallel
// review of issue #605.
export class URL {
  parse(): string {
    return "parsed";
  }

  // Genuinely unused: must STILL be reported even though the class name
  // matches a builtin.
  unusedOnUrl(): void {}
}
