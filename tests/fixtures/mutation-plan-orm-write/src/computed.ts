import { db, records } from "./db";

export async function addComputed(value: number): Promise<number> {
  if (value < 0) return 0;
  await db["insert"](records).values({ value });
  return value;
}
