import type { Brush, Shape, Tool } from "./tool.js";

export class SelectTool implements Tool {
  onDown(): void {}
  onUp(): void {}
}

export class PenBrush implements Brush {
  stroke(): void {}
}

export class SquareShape implements Shape {
  fill(): void {}
}

export class Widget {
  paint(): void {}
  unusedOnWidget(): void {}
}
