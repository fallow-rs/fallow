import type { Kysely } from "kysely";

import type { Row } from "./db";

export async function insertRow(db: Kysely<{ records: Row }>, value: number): Promise<boolean> {
  if (value < 0) return false;
  await db.insertInto("records").values({ value }).execute();
  return true;
}
