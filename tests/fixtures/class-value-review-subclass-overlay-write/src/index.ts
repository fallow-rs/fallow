import { C, Other } from './classes';
class Sub extends C {}
const opaque = () => ({ used(): void {}, dead(): void {}, direct(): void {} });
const consume = (items: { widget: Sub }[], flag: boolean): void => { const saved = items[0]!; if (flag) { saved.widget = opaque(); saved.widget.used(); } };
consume([{ widget: new Sub() }], true);
new C().direct();
new Sub().direct();
new Other().direct();
