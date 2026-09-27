"use client";
import { UserRole, UserShape } from "./iface-mod";

// Issue #2941: a value-syntax import that names only type exports. The import
// is erased at build time, so no finding may be reported.
export function UserBadge(props: { user: UserShape; role: UserRole }) {
  return <span>{props.user.id}</span>;
}
