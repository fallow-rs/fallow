import { Schema } from "effect";

export const makeRule = (key: string) => Schema.Struct({ [key]: Schema.String });

export function makeUnusedRule() {
  return Schema.String;
}

export const makePlain = () => ({ key: "plain" });
