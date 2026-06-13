// A SERVER-ONLY module: importing the `server-only` poison package makes this
// module fail the build if it is ever bundled for the client.
import "server-only";

export function fetchUser() {
  return { id: 1 };
}
