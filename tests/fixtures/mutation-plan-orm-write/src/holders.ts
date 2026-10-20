import { lite } from "./litedb";

const statements = {
  insertMapped: lite.prepare("INSERT INTO t (v) VALUES (?)"),
  findMapped: lite.prepare("SELECT v FROM t WHERE v = ?"),
};

export function addMapped(value: number): number {
  if (value > 1) {
    statements.insertMapped.run(value);
  }
  return value;
}

export function findMapped(value: number): number {
  if (value < 0) return 0;
  return statements.findMapped.get(value) === undefined ? 0 : value;
}

export class PrivateRepository {
  #insertPrivate = lite.prepare("INSERT INTO t (v) VALUES (?)");

  addPrivate(value: number): number {
    if (value > 1) {
      this.#insertPrivate.run(value);
    }
    return value;
  }
}

export class StaticRepository {
  static insertStatic = lite.prepare("INSERT INTO t (v) VALUES (?)");

  static addStatic(value: number): number {
    if (value > 1) {
      StaticRepository.insertStatic.run(value);
    }
    return value;
  }
}
