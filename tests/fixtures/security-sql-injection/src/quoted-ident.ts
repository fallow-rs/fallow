// Negative: a quoted identifier helper may cover identifier positions.
const quoteIdent = (name: string): string => `"${name.replace(/"/g, '""')}"`;

interface Db {
  execute(sql: string): Promise<unknown>;
}

export function countRows(db: Db, table: string): Promise<unknown> {
  return db.execute(`SELECT count(*) FROM ${quoteIdent(table)}`);
}
