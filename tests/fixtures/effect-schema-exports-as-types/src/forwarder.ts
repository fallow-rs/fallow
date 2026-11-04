import { Schema } from "effect";
import { BaseRule, decodeBaseRule, ForwardedError, ForwardedUnion, plainValue } from "./barrel";

export { decodeBaseRule, ForwardedError, ForwardedUnion, plainValue };

export { BaseRule as ReExportedRule } from "./contracts";

export const DerivedRule = BaseRule.pipe(Schema.brand("DerivedRule"));

const IntermediateRule = BaseRule.pipe(Schema.brand("IntermediateRule"));

export const DerivedTwice = IntermediateRule.annotations({ title: "DerivedTwice" });

export const notDerived = plainValue.pipe(Schema.brand("NotDerived"));
