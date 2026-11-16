import { Square } from "./shapes.ts";

export class ProbeSquare extends Square {
  public probe(): number {
    return this.testOnlyHelper();
  }
}

export const probed = new ProbeSquare().probe();
