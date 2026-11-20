import { heldLeak } from "./held-leak";
import { status } from "./schemas";

export class Game {
  readonly latency = status.latency;
  readonly leak = heldLeak;
}
