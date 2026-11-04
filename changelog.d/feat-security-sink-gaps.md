### Added

- **`fallow security` recognizes more callees in existing sink categories.** The new callees are:
  - `command-injection`: `shell.exec` from `shelljs`, and `execFile` and `execFileSync` from `node:child_process`. `execFile` runs no shell, so it reports only when the argument or the module reads untrusted input.
  - `ssti`: `Handlebars.compile`, `pug.render`, `_.template` and `lodash.template`.
  - `ssrf`: `axios.put`, `axios.patch`, `axios.delete`, `axios.head`, a bare `axios()` call with a template, a concatenation or a call result as its URL, the `got` method calls, and `http.get` and `https.get` from the Node modules. An `axios(config)` call stays quiet, and so does an `http.get` that the module does not import from `node:http`.
  - `unsafe-deserialization`: `loadAll` from `js-yaml`.
  - `xxe`: `libxmljs.parseXml`.
  - `jquery-html`: `$.parseHTML` and `jQuery.parseHTML`.
  - `route-send-file`: `res.download` and `response.download` in Express projects.
  - `path-traversal`: named `node:fs` and `node:fs/promises` imports such as `import { readFileSync } from "node:fs"`, and the `fs` write, append, delete and copy calls. These rows report only when the argument or the module reads untrusted input, as the `path.join` row does. A local helper with the same name as an `fs` function stays quiet.

  Existing candidates do not change.
