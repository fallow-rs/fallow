import _ from "lodash";

export function render(source: string): string {
  return _.template(source)({});
}
