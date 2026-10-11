import { C, Other } from './classes';
const consume = (items: ReadonlyArray<C>): void => { items[0]!.used(); };
consume([new C()]);
new C().direct();
new Other().direct();
