import * as ns from './base'
import { UrlSyncManager, mixin } from './base'

class MapUrlSyncManager extends UrlSyncManager {}

class MidUrlSyncManager extends UrlSyncManager {}
class DeepUrlSyncManager extends MidUrlSyncManager {}

// A chain longer than any fixed depth cap. Cycle detection, not a depth bound, is
// what terminates the walk; a cap would abstain here and falsely report
// `viaDeepChain`.
class Deep0 extends UrlSyncManager {}
class Deep1 extends Deep0 {}
class Deep2 extends Deep1 {}
class Deep3 extends Deep2 {}
class Deep4 extends Deep3 {}
class Deep5 extends Deep4 {}
class Deep6 extends Deep5 {}
class Deep7 extends Deep6 {}
class Deep8 extends Deep7 {}
class Deep9 extends Deep8 {}
class Deep10 extends Deep9 {}
class Deep11 extends Deep10 {}
class Deep12 extends Deep11 {}
class Deep13 extends Deep12 {}
class Deep14 extends Deep13 {}
class Deep15 extends Deep14 {}
class Deep16 extends Deep15 {}
class Deep17 extends Deep16 {}

// A mixin superclass is not a bare identifier, so no superclass is recorded and the
// walk abstains. `viaMixin` stays reported: a mixin may redefine what the subclass
// exposes, so crediting through it would be a guess.
class MixedUrlSyncManager extends mixin(UrlSyncManager) {}

// Declares its own static with a base static's name. Crediting the base too is the
// accepted false negative: over-crediting is safe, under-crediting is not.
class ShadowUrlSyncManager extends UrlSyncManager {
  public static shadowedOnSub(value: string): string {
    return value
  }
}

// A namespace-qualified base. The walk re-emits the dotted `ns.UrlSyncManager`
// verbatim, and the analyze layer resolves only bare local names, so this is inert
// and `viaNamespaceBase` stays reported. Pre-existing: a direct
// `ns.UrlSyncManager.viaNamespaceBase()` is equally uncredited on main.
class NamespaceUrlSyncManager extends ns.UrlSyncManager {}

const instance = new MapUrlSyncManager()

export const config = {
  a: MapUrlSyncManager.calledViaSub('x'),
  b: MapUrlSyncManager.passedViaSub,
  c: UrlSyncManager.passedViaBase,
  d: MapUrlSyncManager.arrowViaSub('y'),
  e: DeepUrlSyncManager.viaGrandchild('z'),
  f: instance.instanceViaSub(),
  g: Deep17.viaDeepChain('deep'),
  h: MixedUrlSyncManager.viaMixin('mixed'),
  i: ShadowUrlSyncManager.shadowedOnSub('shadow'),
  j: NamespaceUrlSyncManager.viaNamespaceBase('ns'),
}
