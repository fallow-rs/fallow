import { C } from './c';
const wrap = <T>(get: () => T): { result: T } => ({ result: get() });
const box = wrap(() => new C()); box.result.go();

import { C as ControlC } from './c';
new ControlC().direct();
export const exerciseUnknown = (value: { unknownOnly(): void }): void => { value.unknownOnly(); };
