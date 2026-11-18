import transports from "./transports";
import { codecs } from "./codecs/barrel";
import { LooseParser, StrictParser, strictness } from "./local/parsers";

export const handlesUpgrades = (name: "polling" | "websocket"): boolean =>
  transports[name].prototype.handlesUpgrades;

export const isBinary = codecs.msgpack.prototype.binary;

export const mimeType = codecs.msgpack.prototype.mimeType;

export const jsonIsBinary = codecs["json"].prototype.binary;

export const strict = strictness("strict");

export const parsers = [new StrictParser(), new LooseParser()];
