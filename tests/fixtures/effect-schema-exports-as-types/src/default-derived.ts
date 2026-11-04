import { Schema } from "effect";
import { BaseRule } from "./contracts";

export default BaseRule.pipe(Schema.brand("DefaultDerived"));
