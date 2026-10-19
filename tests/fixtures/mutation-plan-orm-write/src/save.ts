import { db, records } from "./db";

export async function save(value: number): Promise<number> {
  if (value < 0) return 0;
  await db.insert(records).values({ value });
  return value;
}

export function clamp(value: number): number {
  return value < 0 ? 0 : value;
}
