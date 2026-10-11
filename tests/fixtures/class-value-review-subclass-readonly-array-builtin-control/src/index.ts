import { C, Other } from './classes';
class Sub extends C {}
const consume = (items: ReadonlyArray<Sub>): void => { const saved = items[0]!; saved.used(); };
consume([new Sub()]);
new C().direct();
new Sub().direct();
new Other().direct();
