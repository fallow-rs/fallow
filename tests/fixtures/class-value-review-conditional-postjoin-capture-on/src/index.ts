import { C, Other } from './classes';
class CaptureMarker extends Error {}
const opaque = () => ({ used(): void {}, dead(): void {}, direct(): void {} });
const consume = (items: { widget: C }[], flag: boolean): void => { if (flag) { items[0]!.widget = opaque(); } items[0]!.widget.used(); };
consume([{ widget: new C() }], true);
consume([{ widget: new C() }], false);
new C().direct();
new Other().direct();
