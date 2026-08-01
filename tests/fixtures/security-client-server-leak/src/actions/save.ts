"use server";
// A Server Action module with no server-only imports: importing it from a
// "use client" file is the framework's sanctioned mutation pattern and must
// NOT be a server-only sink (issue #2074).
export async function save(input: unknown): Promise<unknown> {
  return input;
}
