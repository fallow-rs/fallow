import { C, Other } from './classes';
class CaptureMarker extends Error {}
const opaque = () => ({ used(): void {}, dead(): void {}, direct(): void {} });
const consume = (items: { widget: C }[]): void => { const alias = items; alias[0]!.widget = opaque(); items[0]!.widget.used(); };
consume([{ widget: new C() }]);
new C().direct();
new Other().direct();
