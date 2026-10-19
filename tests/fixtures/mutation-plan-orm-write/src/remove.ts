import knex from "knex";

const db = knex({ client: "pg" });

export async function removeRecord(id: number): Promise<number> {
  if (id < 0) return 0;
  return db("records").where({ id }).del();
}
