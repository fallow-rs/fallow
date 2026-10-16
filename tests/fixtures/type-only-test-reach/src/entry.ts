import { called } from "./called";
import { shaped } from "./shaped";
import type { Shape } from "./shapes";
import { typed } from "./typed";

export function renderEntry(shape: Shape): string {
  return `${called()} ${shaped()} ${typed()} ${shape.name}`;
}
