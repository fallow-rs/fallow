import { lite } from "./litedb";

const sql = (parts: TemplateStringsArray): string => parts.join("?");

const INSERT_SQL = "INSERT INTO t (v) VALUES (?)";
const insertStmt = lite.prepare("INSERT INTO t (v) VALUES (?)");
const selectStmt = lite.prepare("SELECT v FROM t WHERE v = ?");
const countStmt = lite.prepare(sql`SELECT count(*) FROM t`);

export function addStored(value: number): number {
  if (value < 0) return 0;
  insertStmt.run(value);
  return value;
}

export function addInline(value: number): number {
  if (value < 0) return 0;
  lite.prepare(INSERT_SQL).run(value);
  return value;
}

export function findStored(value: number): number {
  if (value < 0) return 0;
  return selectStmt.get(value) === undefined ? 0 : value;
}

export function countStored(limit: number): number {
  if (limit < 1) return 0;
  return countStmt.all().length > limit ? limit : 1;
}
