import { C } from './c';
const computed = <T>(fn: () => T): { value: T } => ({ value: fn() });
computed(() => new C()).value.go();

import { C as ControlC } from './c';
const direct = new ControlC();
direct.direct();
export const exerciseUnknown = (value: { unknownOnly(): void }): void => { value.unknownOnly(); };
