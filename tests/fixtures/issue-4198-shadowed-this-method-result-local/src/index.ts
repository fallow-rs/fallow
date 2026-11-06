import { Floor } from "./floor.js";
import { Chair, Seat } from "./tables.js";

const makeSeat = (): Seat => new Seat();

export class Viewer extends Floor {
  mark(other: Array<{ reset(): void }>, items: unknown[], chairs: Chair[]): void {
    const table = this.find();
    table?.setVip();
    other.forEach((table) => table.reset());
    for (const table of other) {
      table.reset();
    }
    items.forEach((table) => {
      if (table instanceof Chair) {
        table.narrowed();
      }
    });
    for (const table of chairs) {
      table.rectOnly();
    }
  }
}

export class Usher {
  private readonly seat = new Seat();

  guide(other: Array<{ release(): void }>, items: unknown[]): void {
    const seat = this.current();
    seat.claim();
    other.forEach((seat) => seat.release());
    items.forEach((seat) => {
      if (seat instanceof Seat) {
        seat.narrowed();
      }
    });
    {
      const seat = makeSeat();
      seat.factoryLocal();
    }
  }

  private current(): Seat {
    return this.seat;
  }
}
