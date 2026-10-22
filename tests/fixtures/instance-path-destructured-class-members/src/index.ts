import * as keys from './keys';
import { Svc, shared, spread } from './svc';

const { viaImport } = shared;
viaImport();

let viaImportAssignment: () => void;
({ viaImportAssignment } = shared);
viaImportAssignment();

const runDefault = ({ viaImportDefault } = shared): void => viaImportDefault();
runDefault();

const runTypeof = ({ viaImportTypeof }: typeof shared): void => viaImportTypeof();
runTypeof(shared);

// A rest element copies every member, so no member of `Spread` is unused.
const { ...copy } = spread;
copy.first();

// A dynamic key can select any export of the namespace.
const name = String(Date.now());
const { [name]: picked } = keys;
console.log(picked);

export class Host {
  constructor(private svc: Svc) {}

  run(): void {
    const { viaThis } = this.svc;
    viaThis();
    let viaThisAssignment: () => void;
    ({ viaThisAssignment } = this.svc);
    viaThisAssignment();
  }

  runDefault({ viaThisDefault } = this.svc): void {
    viaThisDefault();
  }
}
