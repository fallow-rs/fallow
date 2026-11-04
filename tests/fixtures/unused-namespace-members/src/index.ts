import { Legacy, Outer, Merged, Color, make, Typed, Whole, Spread, Reexported, Body } from "./legacy";
import x = Legacy.aliased;

export const values = [
  Legacy.used,
  x,
  Outer.Inner.deep,
  Outer.Whole,
  Merged.helper,
  new Merged(),
  Color.parse("red"),
  Color.Red,
  make.defaults,
  make(),
  Body.entry(),
];
const { destructured } = Legacy;
export const d = destructured;
export type T = Typed.Shape;
export const w = Object.keys(Whole);
export const s = { ...Spread };
export { Reexported };
export type A = typeof import("./ambient").Ambient.a;
export * from "./consumer";
