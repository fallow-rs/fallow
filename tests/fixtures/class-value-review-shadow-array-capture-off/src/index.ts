import { C, Other } from './classes';
type Array<T> = Other[];
const consume = (items: Array<C>): void => { items[0]!.used(); };
consume([new Other()]);
new C().direct();
new Other().direct();
