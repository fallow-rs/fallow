import { ArgTable, ChainTable } from "./tables.js";

export class Floor {
  private readonly chain = new ChainTable();

  private readonly tables: ArgTable[] = [new ArgTable()];

  protected find(): ChainTable | undefined {
    return this.chain;
  }

  protected findBy(index: number): ArgTable | undefined {
    return this.tables[index];
  }
}
