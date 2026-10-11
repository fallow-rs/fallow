import { C } from './c';
const consume = (item: { widget: C }): void => { item.widget.go(); };
consume({ widget: new C() });

import { C as ControlC } from './c';
const direct = new ControlC();
direct.direct();
export const exerciseUnknown = (value: { unknownOnly(): void }): void => { value.unknownOnly(); };
