### Fixed

- **A barrel hop in a boundary chain is judged by its own re-export kind.** When a file imports a type through a barrel, and the barrel forwards it with a value re-export that its zone does not allow, the barrel gets the finding and the importer no longer gets a second one.

- **Same-named `autoDiscover` zones with different `root` values keep each root.** Two zones with one name, for example `features` under `packages/a` and under `packages/b`, now both classify their discovered child folders. Before, the files under the second root had no zone.
