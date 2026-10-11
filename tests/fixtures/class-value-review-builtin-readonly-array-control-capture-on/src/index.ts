import { C, Other } from './classes';
class CaptureMarker extends Error {}
const consume = (items: ReadonlyArray<C>): void => { items[0]!.used(); };
consume([new C()]);
new C().direct();
new Other().direct();
