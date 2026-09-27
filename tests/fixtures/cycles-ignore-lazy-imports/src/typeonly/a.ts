import { typeB } from "./b";
export interface Shape {
  n: number;
}
export const typeA = (): Shape => ({ n: typeB({ n: 1 }) });
