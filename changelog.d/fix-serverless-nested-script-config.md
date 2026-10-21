### Fixed

- **A nested `serverless.ts` or `serverless.js` module is no longer read as a
  Serverless config.** The Serverless Framework and osls read `serverless.*`
  from the service directory only. Fallow now reads a JS or TS config only at
  the project root, at a workspace package root, or in a directory with its
  own `package.json`. A module such as `src/lib/serverless.ts` is an ordinary
  source file again: its `plugins` get no dependency credit and its handlers
  are not entry points. A nested `serverless.yml` is still a service config.
