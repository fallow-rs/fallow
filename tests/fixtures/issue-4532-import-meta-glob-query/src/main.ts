import { setup } from "./raw/a";

const options = { query: "?raw" };

const globs = [
	import.meta.glob<string>(["./raw/*.ts", "!./raw/skip.ts"], {
		query: "?raw",
		import: "default",
		eager: true,
	}),
	import.meta.glob("./url/*.ts", { query: "?url" }),
	import.meta.glob("./inline/*.ts", { query: "?inline", eager: true }),
	import.meta.glob("./legacy-raw/*.ts", { as: "raw" }),
	import.meta.glob("./legacy-url/*.ts", { as: "url" }),
	import.meta.glob("./plain/*.ts"),
	import.meta.glob("./defaults/*.ts", { import: "default" }),
	import.meta.glob("./named/*.ts", { import: "setup", eager: true }),
	import.meta.glob("./star/*.ts", { import: "*" }),
	import.meta.glob("./custom/*.ts", { query: "?custom" }),
	import.meta.glob("./dynamic-options/*.ts", options),
	import.meta.glob("./raw-named/*.ts", { query: "?raw", import: "setup" }),
];

console.log(setup, globs);
