import { Effect } from "effect";
import { BaseRule, plainValue } from "./contracts";

export const AliasedRule = BaseRule;

export const AnnotatedRule = BaseRule.annotations({ title: "AnnotatedRule" });

export const aliasedPlain = plainValue;

console.log(Effect.void);
