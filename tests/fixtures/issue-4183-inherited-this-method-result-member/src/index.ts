import { Floor } from "./floor.js";

export class Viewer extends Floor {
  mark(): void {
    const table = this.find();
    table?.setVip();
  }

  close(): void {
    this.find()?.lock();
  }
}
