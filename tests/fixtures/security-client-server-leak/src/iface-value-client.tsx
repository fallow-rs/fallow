"use client";
import { readUsersSecret, UserShape } from "./iface-mod";

// Issue #2941 control: the same module imported for a runtime value stays a
// leak.
export function UserKey(props: { user: UserShape }) {
  return <span>{props.user.id}{readUsersSecret()}</span>;
}
