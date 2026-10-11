import { C } from './c';
const consume = (items: {widget:{go():void}}[]): void => { items[0]!.widget.go(); }; consume([{widget:{go(){}}}]);

import { C as ControlC } from './c';
new ControlC().direct();
export const exerciseUnknown = (value: { unknownOnly(): void }): void => { value.unknownOnly(); };
