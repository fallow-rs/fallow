import { Codes, Level, Service } from "@p/lib";
import { PubCodes, PubLevel, PubService } from "@p/pub";

console.log(Codes.used, Level.Low, new Service().run());
console.log(PubCodes.used, PubLevel.Low, new PubService().run());
