import { C } from './c';
interface Item { widget: C }
const consume = (items: Item[]): void => { items[0]!.widget.go(); }; consume([{widget:new C()}]);

import { C as ControlC } from './c';
new ControlC().direct();
export const exerciseUnknown = (value: { unknownOnly(): void }): void => { value.unknownOnly(); };
