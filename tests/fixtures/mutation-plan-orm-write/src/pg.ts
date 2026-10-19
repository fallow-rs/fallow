import { Pool } from "pg";

const pool = new Pool();

export async function addRow(value: number): Promise<number> {
  if (value < 0) return 0;
  await pool.query("INSERT INTO t (v) VALUES ($1)", [value]);
  return value;
}
