export async function lazySave(value: number): Promise<number> {
  if (value < 0) return 0;
  const { default: knex } = await import("knex");
  await knex({ client: "pg" })("records").insert({ value });
  return value;
}
