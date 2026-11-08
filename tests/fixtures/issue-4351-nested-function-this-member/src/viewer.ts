import { Floor } from "./floor.js";

export class Viewer extends Floor {
  mark(): void {
    function g(this: any): void {
      this.find().setVip();
    }
    g.call({});
    const handlers = {
      run(): void {
        this.find().setVip();
      },
    };
    handlers.run();
  }

  open(): void {
    const visit = (): void => {
      this.pick().lock();
    };
    visit();
  }

  start(): void {
    setTimeout(
      function (this: Viewer): void {
        this.tick();
      }.bind(this),
      0,
    );
  }

  tick(): void {}
}
