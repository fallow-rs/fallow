import { Table } from "./table.js";

export class Floor {
  find(): Table | undefined {
    return new Table();
  }
  pick(): Table {
    return new Table();
  }
  static create(): Table {
    return new Table();
  }
}
