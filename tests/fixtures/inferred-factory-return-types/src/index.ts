import { makeCache, makeCounter, makeReader, makeSettings } from "./factories";

console.log(makeCache().get(), makeReader().read(), makeSettings(), makeCounter().count());
