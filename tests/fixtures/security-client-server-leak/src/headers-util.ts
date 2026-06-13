import { cookies } from "next/headers";
export function readSession(): string | undefined {
  return cookies().get("session")?.value;
}
