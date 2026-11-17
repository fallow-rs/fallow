### Fixed

- **`unused-class-members` follows a Durable Object binding of the Worker `Env`.** A method call on a Durable Object stub now credits the method of the Durable Object class. The stub can come from the `env` import of `cloudflare:workers`, or from `this.env` in a class that extends `DurableObject<Env>`, `WorkerEntrypoint<Env>` or `WorkflowEntrypoint<Env>`. An inline call such as `env.ROOM.getByName(name).join()` also gives credit. Fallow reads the binding types from the `worker-configuration.d.ts` file that `wrangler types` writes. In a monorepo, each Worker package uses the nearest of these files. A stub at the top level of a module gives no credit yet. The extraction cache version goes to 448.
  (Closes [#4548](https://github.com/fallow-rs/fallow/issues/4548))
