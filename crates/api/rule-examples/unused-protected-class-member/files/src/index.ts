import { Shape, Square } from "./shape";

const shapes: Shape[] = [new Square(3)];
console.log(shapes.map((shape) => shape.area()));
