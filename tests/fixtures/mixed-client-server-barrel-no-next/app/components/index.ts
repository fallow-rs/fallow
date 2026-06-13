// Same client+server-only barrel mix as the positive fixture, but the project
// does NOT declare `next`. Without Next.js the "use client" / "use server"
// directives carry no special meaning, so the rule must not fire.
export { Button } from "./Button";
export { fetchUser } from "./fetchUser";
