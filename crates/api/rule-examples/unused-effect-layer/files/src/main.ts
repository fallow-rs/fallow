import { Effect } from "effect";
import { Db, MainLive } from "./services";

const program = Effect.gen(function* () {
  const db = yield* Db;
  return yield* db.query();
});

Effect.runPromise(program.pipe(Effect.provide(MainLive)));
