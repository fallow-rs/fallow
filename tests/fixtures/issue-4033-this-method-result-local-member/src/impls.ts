import type { Brush, Tool } from "./tool.js";

export class SelectTool implements Tool {
  deactivate(): void {}
  activate(): void {}
  reset(): void {}
}

export class PenBrush implements Brush {
  stroke(): void {}
}
