"use server";
import { findUser } from "../users-db";

// Issue #2941: every value export is an async Server Action. The bundler
// replaces the import in a "use client" file with an action reference, so the
// secret read behind this module never enters the client bundle.
export async function loadUser(id: string): Promise<string> {
  return findUser(id);
}

export default async function removeUser(id: string): Promise<void> {
  findUser(id);
}
