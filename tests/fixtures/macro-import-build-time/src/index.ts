import { title } from "./title.macro.ts" with { type: "macro" };
import { paint } from "./paint.ts" with { type: "macro" };
import { report } from "./report.ts";
import { stamp } from "build-stamp" with { type: "macro" };

console.log(title(), paint("x"), report(), stamp());
