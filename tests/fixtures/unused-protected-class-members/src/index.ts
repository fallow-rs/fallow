import { FancyRenderer } from "./fancy-renderer.ts";
import { Extensible } from "./public-api.ts";
import { Shape, Square } from "./shapes.ts";

const shape = new Shape();
const square = new Square();

export const result =
  shape.area() + square.side() + new Extensible().run() + new FancyRenderer().draw();
