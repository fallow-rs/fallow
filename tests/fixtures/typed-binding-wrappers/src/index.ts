import { Aggregate } from './aggregate';
import { AggregateRepo } from './repo';

const repo = new AggregateRepo();

// `Aggregate | undefined` annotation — the dominant nullable repository
// boundary pattern. `rename()` is reached only through this binding.
let pending: Aggregate | undefined;
pending = new Aggregate();
pending.rename();

// `Promise<Aggregate>` annotation. A member access on the Promise object should
// not be credited as an Aggregate member use.
const ready: Promise<Aggregate> = Promise.resolve(new Aggregate());
ready.archive();

void repo.findById('id');
