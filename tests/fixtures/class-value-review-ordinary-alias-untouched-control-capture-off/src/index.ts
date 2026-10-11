import { C, Other } from './classes';
const consume = (items: { widget: C }[]): void => { const alias = items; alias[0]!.widget.used(); };
consume([{ widget: new C() }]);
new C().direct();
new Other().direct();
