import Assigned = require('./assigned');
import Narrowed = require('./narrowed');
import Whole = require('./whole');

import * as NarrowedEsm from './narrowed-esm';
import { Outer } from './ns-reexport';
import { aliasedMember } from './local-alias';

console.log(Assigned.viaAssignment);
console.log(Narrowed.used);
console.log(NarrowedEsm.esmUsed);
console.log(Object.values(Whole));
console.log(Outer.outerValue);
console.log(aliasedMember);
