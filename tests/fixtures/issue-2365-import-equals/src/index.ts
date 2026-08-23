import Assigned = require('./assigned');
import Narrowed = require('./narrowed');
import Typed = require('./typed');
import Whole = require('./whole');
import Destructured = require('./destructured');

import * as NarrowedEsm from './narrowed-esm';
import * as TypedEsm from './typed-esm';
import * as DestructuredEsm from './destructured-esm';
import * as EntryReexportEsm from './entry-reexport-esm';
import { readRuntime, readTypeOnly, readTypeOnlyEsm } from './deps';
import { aliasedMember } from './local-alias';
import { staleMarker } from './stale';
import { staleEsmMarker } from './stale-esm';
import { parseShadowed, readShadowed } from './shadowed';
import { parseShadowedEsm, readShadowedEsm } from './shadowed-esm';

// The entry re-exports the binding itself, so consumers the graph cannot
// enumerate reach every name on the required module object.
export import EntryReexport = require('./entry-reexport');

export { EntryReexportEsm };

const { destructuredUsed } = Destructured;
const { esmDestructuredUsed } = DestructuredEsm;

export const readTyped = (shape: Typed.UsedShape): number => shape.n + Typed.typedValue;
export const readTypedEsm = (shape: TypedEsm.EsmUsedShape): number =>
  shape.n + TypedEsm.esmTypedValue;

console.log(Assigned.viaAssignment);
console.log(Narrowed.used);
console.log(NarrowedEsm.esmUsed);
console.log(Object.values(Whole));
console.log(readRuntime, readTypeOnly, readTypeOnlyEsm);
console.log(aliasedMember);
console.log(destructuredUsed, esmDestructuredUsed);
console.log(staleMarker, staleEsmMarker);
console.log(readShadowed(), parseShadowed({ n: 1 }));
console.log(readShadowedEsm(), parseShadowedEsm({ n: 1 }));
