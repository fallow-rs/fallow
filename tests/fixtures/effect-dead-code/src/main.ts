import { Effect } from "effect"
import { AppLive, Db, MainLive } from "./services"
import { Forbidden, NotFound, type NeverThrown, type Silenced } from "./errors"
import * as Errors from "./errors"
import ViaDefaultError from "./default-error"
import { nsErrors } from "./errors-index"

const load = (id: string): Effect.Effect<number, NotFound | NeverThrown | Silenced, Db> =>
  Effect.gen(function* () {
    const db = yield* Db
    const n = yield* db.query()
    if (n === 0) return yield* new NotFound({ id })
    if (n === 1) return yield* new Errors.ViaNamespace()
    if (n === 2) return yield* new ViaDefaultError()
    if (n === 3) return yield* new nsErrors.ViaNamespaceReExport()
    return n
  })

const program = load("a").pipe(
  Effect.catchTag("NotFound", () => Effect.succeed(0)),
  Effect.catchTag("NeverThrown", () => Effect.succeed(-1)),
  Effect.catchTag("Silenced", () => Effect.succeed(-3)),
  Effect.catchTag("TimeoutException", () => Effect.succeed(-4)),
  Effect.catchTag("FromPackage", () => Effect.succeed(-5)),
  Effect.catchTag("ViaDefault", () => Effect.succeed(-6)),
  Effect.catchTag("ViaNamespaceReExport", () => Effect.succeed(-7)),
  Effect.catchTags({ Forbidden: () => Effect.succeed(-2) }),
)

export const forbidden = Forbidden

Effect.runPromise(program.pipe(Effect.provide(MainLive), Effect.provide(AppLive)))

export { PublicLive, RenamedPublicLive } from "./services"
export { PublicError } from "./errors"
