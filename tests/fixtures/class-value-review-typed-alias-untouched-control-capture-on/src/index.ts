import { C, Other } from './classes';
class CaptureMarker extends Error {}
const consume = (items: { widget: C }[]): void => { const alias: { widget: C }[] = items; alias[0]!.widget.used(); };
consume([{ widget: new C() }]);
new C().direct();
new Other().direct();
