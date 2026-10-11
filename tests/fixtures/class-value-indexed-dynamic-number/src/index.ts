import { C } from './c';
const consume = (items: {widget:C}[], index: number): void => { items[index]!.widget.go(); }; consume([{widget:new C()}],0);

import { C as ControlC } from './c';
new ControlC().direct();
export const exerciseUnknown = (value: { unknownOnly(): void }): void => { value.unknownOnly(); };
