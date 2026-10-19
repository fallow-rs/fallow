import { Kysely } from "kysely";

declare const db: Kysely<{ person: { name: string } }>;

export async function addPerson(name: string): Promise<boolean> {
  if (name.length === 0) return false;
  await db.insertInto("person").values({ name }).execute();
  return true;
}
