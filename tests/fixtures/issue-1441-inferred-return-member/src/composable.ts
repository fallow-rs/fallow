import { Api } from './api'

// Inferred return type (NO `: Api` annotation): the class type reaches the
// consumer only through the typed module-local `let api: Api` this returns.
// This is the exact #1441 repro; the #1634 fixtures all annotate the return.
let api: Api
export function useApi() {
  if (!api) {
    api = new Api()
  }
  return api
}
