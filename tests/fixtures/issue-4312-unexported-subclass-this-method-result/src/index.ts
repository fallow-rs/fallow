import { Floor } from "./floor.js";

class Viewer extends Floor {
  mark(): void {
    this.find()?.setVip();
  }
}

class Gallery extends Viewer {
  close(): void {
    this.find()?.lock();
  }
}

new Viewer().mark();
new Gallery().close();
