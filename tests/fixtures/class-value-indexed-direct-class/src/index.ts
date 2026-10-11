import { C } from './c';
const consume = (items: C[]): void => { items[0]!.go(); }; consume([new C()]);

import { C as ControlC } from './c';
new ControlC().direct();
export const exerciseUnknown = (value: { unknownOnly(): void }): void => { value.unknownOnly(); };
