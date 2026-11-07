import type { Point } from "./point.js";

export function toWorld(local: Point): Point {
  return local.rotate(1).translate(1, 1);
}

export function grow(local: Point): Point {
  return local.rotate(1).translate(2, 2).scale(3);
}
