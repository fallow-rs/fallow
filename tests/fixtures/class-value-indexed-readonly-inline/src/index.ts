import { C } from './c';
const consume = (items: ReadonlyArray<{ widget: C }>): void => { items[0]!.widget.go(); }; consume([{widget:new C()}]);

import { C as ControlC } from './c';
new ControlC().direct();
export const exerciseUnknown = (value: { unknownOnly(): void }): void => { value.unknownOnly(); };
