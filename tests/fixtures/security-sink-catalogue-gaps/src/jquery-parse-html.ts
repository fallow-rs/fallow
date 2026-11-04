import $ from "jquery";

export function build(markup: string): unknown {
  return $.parseHTML(markup);
}
