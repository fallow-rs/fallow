// Package-script tooling stays reachable for dead-code analysis, but its
// imports do not prove that a devDependency ships with the application.
import { optimize } from "svgo";

export const build = (source: string): string => optimize(source).data;
