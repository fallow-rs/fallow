import { get } from "./get";

export function pick(object: Record<string, unknown>, keys: string[]): unknown[] {
  return keys.map((key) => get(object, key + ""));
}
