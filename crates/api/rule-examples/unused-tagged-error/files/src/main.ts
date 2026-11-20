import { Effect } from "effect";
import { NotFound, type Expired } from "./errors";

const loadCart = (id: string): Effect.Effect<number, NotFound | Expired> =>
  id === "" ? Effect.fail(new NotFound({ id })) : Effect.succeed(3);

const program = loadCart("cart-1").pipe(
  Effect.catchTag("NotFound", () => Effect.succeed(0)),
  Effect.catchTag("Expired", () => Effect.succeed(-1)),
);

Effect.runPromise(program);
