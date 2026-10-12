import { releaseChannel } from "../src/release.mjs";

console.log(releaseChannel(process.argv[2] ?? "1.0.0"));
