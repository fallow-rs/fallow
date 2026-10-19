import postgres from "postgres";

const sql = postgres();

export async function addUser(name: string): Promise<number> {
  if (name.length === 0) return 0;
  await sql`insert into users (name) values (${name})`;
  return 1;
}

export async function countUsers(limit: number): Promise<number> {
  if (limit < 0) return 0;
  const rows = await sql`select count(*) from users`;
  return rows.length;
}
