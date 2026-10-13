import { Context, Effect, Layer } from "effect"

export class Db extends Context.Service<Db, { readonly query: () => Effect.Effect<number> }>()("app/Db") {
  static readonly layer = Layer.succeed(this, { query: () => Effect.succeed(1) })
}

export class Cache extends Context.Service<Cache, { readonly get: () => Effect.Effect<number> }>()("app/Cache") {
  static readonly layer = Layer.effect(
    this,
    Effect.gen(function* () {
      const db = yield* Db
      return { get: () => db.query() }
    }),
  )
}

const CacheLive = Cache.layer.pipe(Layer.provide(Db.layer))

export const OrphanLive = Layer.mergeAll(CacheLive)

const RenamedLive = Layer.succeed(Db, { query: () => Effect.succeed(3) })

const AliasedAppLive = Layer.mergeAll(RenamedLive)

export { AliasedAppLive as AppLive }

export const MainLive = Layer.mergeAll(Db.layer)

const InnerLive = Layer.succeed(Db, { query: () => Effect.succeed(4) })

export const PublicLive = Layer.mergeAll(InnerLive)

const DeepLive = Layer.succeed(Db, { query: () => Effect.succeed(5) })

const AliasedPublicLive = Layer.mergeAll(DeepLive)

export { AliasedPublicLive as RenamedPublicLive }
