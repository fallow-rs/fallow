import { Context, Effect, Layer } from "effect";

export class Db extends Context.Tag("shop/Db")<Db, { readonly query: () => Effect.Effect<number> }>() {
  static readonly layer = Layer.succeed(this, { query: () => Effect.succeed(1) });
}

class Cache extends Context.Tag("shop/Cache")<Cache, { readonly get: () => Effect.Effect<number> }>() {
  static readonly layer = Layer.effect(
    this,
    Effect.gen(function* () {
      const db = yield* Db;
      return { get: () => db.query() };
    }),
  );
}

const CacheLive = Cache.layer.pipe(Layer.provide(Db.layer));

const OrphanLive = Layer.mergeAll(CacheLive);

export const MainLive = Layer.mergeAll(Db.layer);
