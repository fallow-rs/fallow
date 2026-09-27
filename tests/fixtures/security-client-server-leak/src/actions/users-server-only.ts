"use server";
import "server-only";
import { findUser } from "../users-db";

// Issue #2941: an all-action module that also imports the `server-only`
// package. The action boundary stops the client cone, so neither the
// server-only import nor the secret read behind it is a finding.
export const loadUserGuarded = async (id: string): Promise<string> => findUser(id);

async function renameUser(id: string): Promise<string> {
  return findUser(id);
}

export { renameUser };

export type UserId = string;
