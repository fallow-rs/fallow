import { Table } from "./table.js";

export class Floor {
  protected find(): Table | undefined {
    return new Table();
  }
}
