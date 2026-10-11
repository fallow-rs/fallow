import { C, Other } from './classes';
class CaptureMarker extends Error {}
type ReadonlyArray<T> = readonly Other[];
const consume = (items: ReadonlyArray<C>): void => { items[0]!.used(); };
consume([new Other()]);
new C().direct();
new Other().direct();
