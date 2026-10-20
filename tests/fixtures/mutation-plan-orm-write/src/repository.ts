import { lite } from "./litedb";

let clearStmt;
clearStmt = lite.prepare("DELETE FROM t");

export function clearAssigned(force: boolean): boolean {
  if (!force) return false;
  clearStmt.run();
  return true;
}

export class Repository {
  private insertStmt = lite.prepare("INSERT INTO t (v) VALUES (?)");
  private findStmt = lite.prepare("SELECT v FROM t WHERE v = ?");
  private updateStmt;

  constructor() {
    this.updateStmt = lite.prepare("UPDATE t SET v = ?");
  }

  addField(value: number): number {
    if (value > 1) {
      this.insertStmt.run(value);
    }
    return value;
  }

  setField(value: number): number {
    if (value > 1) {
      this.updateStmt.run(value);
    }
    return value;
  }

  findField(value: number): number {
    if (value < 0) return 0;
    return this.findStmt.get(value) === undefined ? 0 : value;
  }
}
