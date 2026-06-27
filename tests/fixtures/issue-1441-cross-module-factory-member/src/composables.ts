import { RESTApi } from './rest-api'

// Typed module-local return: `useApi` returns the bare module `api`. The class is
// VALUE-proven by the in-body assignment `api = initializeApi()` (initializeApi is
// a same-file direct-new factory) , NOT by the `: RESTApi` type annotation alone.
// A consumer's `const a = useApi(); a.Plan()` must credit RESTApi.Plan across the
// module boundary. Issue #1441 (Part A).
let api: RESTApi
function initializeApi(): RESTApi {
  return new RESTApi()
}
export function useApi(): RESTApi {
  if (!api) {
    api = initializeApi()
  }
  return api
}

// Direct `new RESTApi()` return (re-exported through barrel.ts).
export function makeDirect(): RESTApi {
  return new RESTApi()
}

// Aliased export of a direct-new factory: published under `useAliased`.
function makeAliased(): RESTApi {
  return new RESTApi()
}
export { makeAliased as useAliased }

// NOT a class factory (returns a plain object). A consumer's
// `const n = notAFactory(); n.Ghost()` must NOT credit RESTApi.Ghost.
export function notAFactory(): any {
  return {}
}
