import Database from "better-sqlite3";

const lite = new Database("local.db");

export function insertValue(value: number): number {
  if (value < 0) return 0;
  lite.prepare("INSERT INTO t (v) VALUES (?)").run(value);
  return value;
}

export function clearValues(force: boolean): boolean {
  if (!force) return false;
  lite.exec("DELETE FROM t");
  return true;
}

const insert = lite.prepare("INSERT INTO t (v) VALUES (?)");

export function insertPrepared(value: number): number {
  if (value < 0) return 0;
  insert.run(value);
  return value;
}
