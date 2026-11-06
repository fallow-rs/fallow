import { DeepTable, LocalTable } from "./tables.js";
import { Floor } from "./floor.js";

class LocalFloor {
  private readonly table = new LocalTable();

  protected find(): LocalTable | undefined {
    return this.table;
  }
}

export class LocalViewer extends LocalFloor {
  mark(): void {
    const table = this.find();
    table?.setVip();
  }

  close(): void {
    this.find()?.lock();
  }
}

class Mid extends Floor {}

export class ChainViewer extends Mid {
  mark(): void {
    const table = this.find();
    table?.setVip();
  }

  close(): void {
    this.find()?.lock();
  }
}

class DeepFloor {
  private readonly table = new DeepTable();

  protected find(): DeepTable | undefined {
    return this.table;
  }
}

class DeepMid extends DeepFloor {}

export class DeepViewer extends DeepMid {
  mark(): void {
    const table = this.find();
    table?.setVip();
  }

  close(): void {
    this.find()?.lock();
  }
}

export class ArgViewer extends Floor {
  mark(): void {
    const table = this.findBy(0);
    table?.setVip();
  }

  close(): void {
    this.findBy(0)?.lock();
  }
}
