import { called } from "./called";
import { shaped } from "./shaped";
import { typed } from "./typed";

export function renderEntry(): string {
  return `${called()} ${shaped()} ${typed()}`;
}
