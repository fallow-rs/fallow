import { Barrel, Escaped, Home, Outer, Passed } from "@p/lib";
import * as LibRoot from "@p/lib";
import { Handed, Local } from "./local-barrel.ts";

const inspect = (value: unknown): void => {
  console.log(value);
};

console.log(Home.used);
console.log(Barrel.barrelUsed);
console.log(Outer.outerUsed, Outer.Inner.innerUsed);
console.log(LibRoot.Lib.libUsed);
console.log(Object.keys(Escaped));
inspect(Passed);
console.log(Local.Deep.deepRead);
inspect(Handed);
