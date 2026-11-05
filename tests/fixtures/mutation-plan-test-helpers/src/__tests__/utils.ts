import { add } from "../add.js";

export const sum3 = (a: number, b: number, c: number): number => add(add(a, b), c);
