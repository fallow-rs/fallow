import { C, Other } from './classes';
class Sub extends C {}
type Array<T> = Other[];
const consume = (items: Array<Sub>): void => { const saved = items[0]!; saved.used(); };
consume([new Other()]);
new C().direct();
new Sub().direct();
new Other().direct();
