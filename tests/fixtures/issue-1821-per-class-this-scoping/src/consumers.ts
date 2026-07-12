import { DepPrivParam, DepPubField, DepHashA, DepHashB } from './dep';

// Two classes in ONE file both name their receiver field `dep`. Before
// per-class scoping the module-flat `this.dep` binding collided
// (last-write-wins), so only the class declared last credited its dep's
// members and the other's real member was falsely reported unused (issue
// #1821, Fix B).
export class ConsumerPrivParam {
  constructor(private dep: DepPrivParam) {}

  run(): void {
    this.dep.privParam();
  }
}

export class ConsumerPubField {
  readonly dep = new DepPubField();

  run(): void {
    this.dep.pubField();
  }
}

// The same collision through `#`-private fields: both classes name the field
// `#dep`.
export class ConsumerHashA {
  readonly #dep = new DepHashA();

  run(): void {
    this.#dep.hashA();
  }
}

export class ConsumerHashB {
  readonly #dep = new DepHashB();

  run(): void {
    this.#dep.hashB();
  }
}
