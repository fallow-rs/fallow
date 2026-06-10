// Positive: SQL identifier quoting is not SQL value parameterization.
const quoteIdent = (name: string): string => `"${name.replace(/"/g, '""')}"`;

interface Db {
  execute(sql: string): Promise<unknown>;
}

export function lookupOwner(db: Db, owner: string): Promise<unknown> {
  return db.execute(`SELECT * FROM owners WHERE name = ${quoteIdent(owner)}`);
}
