import { C, Other } from './classes';
const consume = (items: Array<C>): void => { items[0]!.used(); };
consume([new C()]);
new C().direct();
new Other().direct();
