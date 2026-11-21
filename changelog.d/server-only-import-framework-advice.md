### Fixed

- **A React Router `.server` sink gets React Router advice.** The `evidence` and the `Next:` step of a `server-only-import` candidate whose sink is a React Router `.server` module no longer talk about `next/dynamic`, Server Actions or `"use server"` modules. The text now says that React Router fails the client build for this import, and that the fix is to move the call into `loader`, `action`, `middleware` or `headers`, or to move the code that the client needs out of the `.server` module. Sinks that import `server-only`, `next/headers`, `node:fs` or other server-only code keep the Next.js text. Thanks to the reporter for the clear reproduction.
  (Closes [#4954](https://github.com/fallow-rs/fallow/issues/4954))
