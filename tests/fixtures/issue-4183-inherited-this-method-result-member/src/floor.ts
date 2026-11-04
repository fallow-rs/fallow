import { RectTable, RoundTable, type Table } from "./tables.js";

export class Floor {
  private readonly tables: Table[] = [new RectTable(), new RoundTable()];

  protected find(): Table | undefined {
    return this.tables[0];
  }
}
