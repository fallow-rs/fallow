import { C, Other } from './classes';
class CaptureMarker extends Error {}
const consume = (items: { widget: C }[], flag: boolean): void => { if (flag) { void items.length; } else { items[0]!.widget.used(); } };
consume([{ widget: new C() }], true);
consume([{ widget: new C() }], false);
new C().direct();
new Other().direct();
