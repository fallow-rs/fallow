### Fixed

- **`unused-class-members` follows a Durable Object binding of the Worker `Env`.** A method call on a stub of a Durable Object namespace now credits the method of the Durable Object class when the stub comes from the `env` import of `cloudflare:workers`, from `this.env` in a class that extends `DurableObject<Env>`, `WorkerEntrypoint<Env>` or `WorkflowEntrypoint<Env>`, or from an inline call such as `env.ROOM.getByName(name).join()`. Fallow reads the binding types from the `worker-configuration.d.ts` file that `wrangler types` writes: a global `Env` that extends a base interface, with bindings typed `DurableObjectNamespace<import("./src/room").Room>`. The extraction cache version goes to 447.
  (Closes [#4548](https://github.com/fallow-rs/fallow/issues/4548))
