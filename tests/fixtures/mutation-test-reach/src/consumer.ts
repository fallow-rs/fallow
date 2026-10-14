import { price } from "./service";

export function total(value: number): number {
  return price(value) - 1;
}
