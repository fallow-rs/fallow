import Handlebars from "handlebars";

export function render(source: string): string {
  return Handlebars.compile(source)({});
}
