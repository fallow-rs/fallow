### Fixed

- **`invalid-client-export` reports only App Router entry files.** Next.js checks the exports of a `"use client"` module only in a `page`, `layout` or `route` file with a default page extension (`.js`, `.jsx`, `.ts`, `.tsx`) below `app/` or `src/app/`. Another client module, for example a `page-client.tsx` that a `page.tsx` imports, can export `dynamic` or `metadata`, and the build accepts it. A route HTTP method name (`GET`, `POST`, ...) is reported only in a `route` file, and a Pages Router file is not reported.
  (Closes [#5405](https://github.com/fallow-rs/fallow/issues/5405))
