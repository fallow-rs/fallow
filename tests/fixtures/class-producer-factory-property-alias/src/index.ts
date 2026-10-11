import { createUi } from './factory';
const ui = createUi();
ui.page.go();

import { C as ControlC } from './c';
const direct = new ControlC();
direct.direct();
export const exerciseUnknown = (value: { unknownOnly(): void }): void => { value.unknownOnly(); };
