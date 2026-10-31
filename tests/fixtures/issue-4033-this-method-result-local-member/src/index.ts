import { PenBrush, SelectTool } from "./impls.js";
import type { Brush, Tool } from "./tool.js";

export class Manager {
  private tool: Tool | undefined = new SelectTool();
  private brush: Brush | undefined = new PenBrush();

  switchTool(): void {
    const old = this.current();
    old?.deactivate?.();
    const next = this.current?.();
    next?.activate?.();
    const pen = this.#pen()!;
    pen.stroke?.();
  }

  private current(): Tool | undefined {
    return this.tool;
  }

  #pen(): Brush | undefined {
    return this.brush;
  }
}
