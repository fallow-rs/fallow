import Assigned = require('./assigned');
import Narrowed = require('./narrowed');
import Typed = require('./typed');
import Whole = require('./whole');
import Destructured = require('./destructured');

import * as NarrowedEsm from './narrowed-esm';
import * as TypedEsm from './typed-esm';
import * as DestructuredEsm from './destructured-esm';
import * as EntryReexportEsm from './entry-reexport-esm';
import { Outer } from './ns-reexport';
import { aliasedMember } from './local-alias';

// The entry re-exports the binding itself, so consumers the graph cannot
// enumerate reach every name on the required module object.
export import EntryReexport = require('./entry-reexport');

export namespace EntryApi {
  export import EntryNs = require('./entry-ns');
}

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
console.log(Outer.outerValue);
console.log(aliasedMember);
console.log(destructuredUsed, esmDestructuredUsed);
