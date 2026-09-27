"use client";
import removeUser, { loadUser } from "./actions/users";

// Issue #2941: a client that calls Server Actions. No finding may be reported.
export function UserCard() {
  return <button onClick={() => { loadUser("1"); removeUser("1"); }}>Load</button>;
}
