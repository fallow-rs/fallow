// Production entry point. Runtime (value) import of a devDependency: `yaml`
// should be flagged as dev-dependency-in-production (promote to dependencies).
import { parse } from "yaml";
import { pad } from "./lib/pad";

export const load = (text: string): unknown => pad(parse(text));
