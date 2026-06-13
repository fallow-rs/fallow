// Importing the `server-only` poison package marks this module server-only: the
// build fails if it is ever bundled for the client.
import "server-only";
export function loadServerData(): string {
  return "server-data";
}
