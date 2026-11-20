import "server-only";

export async function getUser(): Promise<{ name: string }> {
  return { name: "Ada" };
}
