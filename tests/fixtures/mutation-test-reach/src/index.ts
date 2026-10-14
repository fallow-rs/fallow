import { total } from "./consumer";
import { add } from "./math";

export function main(value: number): number {
  return total(value) * add(value, 1);
}
