import { Floor } from "./floor.js";
import { Seat } from "./tables.js";

export class Viewer extends Floor {
  mark(other: Array<{ reset(): void }>): void {
    const table = this.find();
    table?.setVip();
    other.forEach((table) => table.reset());
    for (const table of other) {
      table.reset();
    }
  }
}

export class Usher {
  private readonly seat = new Seat();

  guide(other: Array<{ release(): void }>): void {
    const seat = this.current();
    seat.claim();
    other.forEach((seat) => seat.release());
  }

  private current(): Seat {
    return this.seat;
  }
}
