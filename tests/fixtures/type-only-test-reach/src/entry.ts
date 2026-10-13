import { called } from "./called";
import { typed } from "./typed";

export function renderEntry(): string {
  return `${called()} ${typed()}`;
}
