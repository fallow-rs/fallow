### Fixed

- **A `package.json` script that runs a binary through its `node_modules/.bin` path now uses the package of that binary.** Before, a script such as `./node_modules/.bin/kysely-codegen --out-file db.d.ts` reported `kysely-codegen` as an unused dependency, and `fallow fix` removed it. A bare `kysely-codegen` call was correct. These forms now give the same credit as the bare name:
  - `./node_modules/.bin/<name>`, `node_modules/.bin/<name>` and `../../node_modules/.bin/<name>`;
  - a Node.js runner with that path as its program, for example `node ./node_modules/.bin/<name>`, `node -r ./setup.js node_modules/.bin/<name>` or `./node_modules/.bin/tsx node_modules/.bin/<name>`;
  - `$(npm bin)/<name>` and `` `npm bin`/<name> ``.

  When the path has a `.bin` link, the package that the link points to gets the credit.
