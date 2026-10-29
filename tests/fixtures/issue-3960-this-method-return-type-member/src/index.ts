import { PenBrush, SelectTool, SquareShape, Widget } from "./impls.js";
import type { Brush, Shape, Tool } from "./tool.js";

export class Editor {
  private readonly tool: Tool = new SelectTool();
  private readonly brush: Brush = new PenBrush();
  private readonly shape: Shape = new SquareShape();
  private readonly widgetInstance = new Widget();

  press(): void {
    this.current()?.onDown?.();
    this.#pen()?.stroke?.();
    this.activeShape?.fill?.();
    this.widget().paint();
  }

  private current(): Tool | undefined {
    return this.tool;
  }

  #pen(): Brush | undefined {
    return this.brush;
  }

  private get activeShape(): Shape | undefined {
    return this.shape;
  }

  private widget(): Widget {
    return this.widgetInstance;
  }
}
