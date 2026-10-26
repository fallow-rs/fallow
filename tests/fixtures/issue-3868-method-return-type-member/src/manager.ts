import type { Tool } from "./tool.js";
import { Widget } from "./widget.js";

export class Manager {
  private tool: Tool | undefined;
  private readonly view = new Widget();

  register(tool: Tool): void {
    this.tool = tool;
  }

  current(): Tool | undefined {
    return this.tool;
  }

  widget(): Widget {
    return this.view;
  }
}
