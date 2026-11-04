import yaml from "js-yaml";

export function parse(text: string): unknown {
  return yaml.loadAll(text);
}
