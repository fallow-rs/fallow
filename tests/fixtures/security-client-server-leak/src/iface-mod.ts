// Issue #2941: a module that exports an interface next to a function that
// reads a non-public env secret.
export interface UserShape {
  id: string;
}

export type UserRole = "admin" | "member";

export function readUsersSecret(): string | undefined {
  return process.env.USERS_API_KEY;
}
