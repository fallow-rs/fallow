import { getArrowBox } from "./arrow";
import { callStatus } from "./call";
import defaultStatus from "./default";
import { FieldHolder } from "./field";
import { getFunctionStatus, getShadowed } from "./function";
import { memberStatus } from "./member";

export class Game {
  readonly arrow = getArrowBox();
  readonly call = callStatus.latency;
  readonly default = defaultStatus.latency;
  readonly field = new FieldHolder().latency;
  readonly function = getFunctionStatus().latency;
  readonly shadowed = getShadowed();
  readonly member = memberStatus.latency;
}
