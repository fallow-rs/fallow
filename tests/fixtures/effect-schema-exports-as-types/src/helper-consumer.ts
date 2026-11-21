import { Schema } from "effect";
import { makePlain, makeRule } from "./helper-barrel";

export const FromImportedHelper = makeRule("a");
export const FromImportedHelperPipe = makeRule("b").pipe(Schema.annotations({ title: "B" }));

const wrapRule = () => makeRule("c");

export const FromWrappedHelper = wrapRule();

export const fromImportedPlain = makePlain();
export const importedHelperAlias = makeRule;
