import { Renderer } from "./renderer.ts";

export class FancyRenderer extends Renderer {
  protected override paint(): number {
    return 1;
  }
}
