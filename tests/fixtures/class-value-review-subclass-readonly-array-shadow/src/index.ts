import { C, Other } from './classes';
class Sub extends C {}
type ReadonlyArray<T> = readonly Other[];
const consume = (items: ReadonlyArray<Sub>): void => { const saved = items[0]!; saved.used(); };
consume([new Other()]);
new C().direct();
new Sub().direct();
new Other().direct();
