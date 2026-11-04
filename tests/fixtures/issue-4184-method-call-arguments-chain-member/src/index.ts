import { Point } from "./point.js";

const local = new Point(0, 0);
console.log(local.rotate(1).translate(1, 1));
console.log(local.rotate(1).translate(2, 2).scale(3));
console.log(local.label("origin").render());
