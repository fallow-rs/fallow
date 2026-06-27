// Internal class (exported from a non-entry module, never re-exported by the
// entry), so its members are subject to unused-class-member detection. Members
// are reached cross-module ONLY through the factory wrappers in composables.ts:
//   Plan     -> via useApi()      (typed module-local return)
//   Material -> via makeDirect()  (direct `new RESTApi()` return, through a barrel)
//   Settings -> via useAliased()  (aliased export of a direct-new factory)
//   Ghost    -> reached only as notAFactory().Ghost(); notAFactory is NOT a class
//               factory, so Ghost must STAY flagged (cross-module over-credit guard)
//   unusedMethod -> never reached; baseline dead member
export class RESTApi {
  Plan(): number {
    return 1
  }

  Material(): number {
    return 2
  }

  Settings(): number {
    return 3
  }

  Ghost(): number {
    return 4
  }

  unusedMethod(): number {
    return 5
  }
}
