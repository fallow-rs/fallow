"use server";
// A Server Action module: the "use server" directive marks it server-only.
export async function saveRecord(value: string): Promise<string> {
  return value.trim();
}
