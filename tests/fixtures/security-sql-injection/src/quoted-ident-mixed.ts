// Positive: quoted identifiers do not cover an unsanitized SQL value fragment.
const quoteIdent = (name: string): string => `"${name.replace(/"/g, '""')}"`;

interface Db {
  execute(sql: string): Promise<unknown>;
}

export function countRows(
  db: Db,
  table: string,
  owner: string,
): Promise<unknown> {
  return db.execute(
    `SELECT count(*) FROM ${quoteIdent(table)} WHERE owner = ${owner}`,
  );
}
