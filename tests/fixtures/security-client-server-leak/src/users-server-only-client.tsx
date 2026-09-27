"use client";
import { loadUserGuarded, renameUser } from "./actions/users-server-only";

// Issue #2941: a client that calls actions from a module with
// `import "server-only"`. No finding may be reported.
export function GuardedUserCard() {
  return <button onClick={() => { loadUserGuarded("1"); renameUser("1"); }}>Load</button>;
}
