import * as keys from './keys';
import { Field, Shadowed, Svc, quiet, shared, spread } from './svc';

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

// A parameter or a local named `quiet` shadows the import, so these
// destructures read no member of `Shadowed`, and `hidden` stays unused.
quiet.used();
export const held: Shadowed = quiet;
export function shadowRest(quiet: { hidden: number }): object {
  const { ...copied } = quiet;
  return copied;
}
export function shadowComputed(): number {
  const quiet = { hidden: 1 };
  const { [name]: value } = quiet;
  return value;
}

// A rest element of `this.field` copies every member of `Field`.
export class FieldHost {
  constructor(private field: Field) {}

  run(): object {
    const { ...copied } = this.field;
    return copied;
  }
}
