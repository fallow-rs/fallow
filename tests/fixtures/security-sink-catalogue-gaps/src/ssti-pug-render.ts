import pug from "pug";

export function render(source: string): string {
  return pug.render(source);
}
