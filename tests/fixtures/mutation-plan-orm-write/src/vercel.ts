import { sql } from "@vercel/postgres";

export async function addValue(value: number): Promise<number> {
  if (value < 0) return 0;
  await sql`
    INSERT INTO t (v) VALUES (${value})`;
  return value;
}
