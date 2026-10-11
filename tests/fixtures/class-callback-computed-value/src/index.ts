import { C } from './c';
const computed = <T>(fn: () => T): { value: T } => ({ value: fn() });
const box = computed(() => new C());
box.value.go();

import { C as ControlC } from './c';
const direct = new ControlC();
direct.direct();
export const exerciseUnknown = (value: { unknownOnly(): void }): void => { value.unknownOnly(); };
