import * as ns from './c';
class Sub extends ns.C {}
Sub.positive();
const value = new Sub(); value.instance();
const shadow = (Sub: { shadowOnly(): void }): void => { Sub.shadowOnly(); };
shadow({ shadowOnly() {} });
export const unknown = (value: { unknownOnly(): void }): void => { value.unknownOnly(); };

const local = (): void => {
  const inner = new Sub();
  inner.functionLocalInstance();
};
local();

const typed = (input: Sub): void => { input.typedParameter(); };
typed(new Sub());
const alias = value;
alias.aliasInstance();
const holder = { item: new Sub() };
holder.item.heldInstance();

{
  const Sub = { localStatic: (): void => {} };
  Sub.localStatic();
  const value = { localInstance: (): void => {} };
  value.localInstance();
}
Sub.restoredLocal();
const restClass = (...[Sub]: [{ restStatic(): void }]): void => { Sub.restStatic(); };
restClass({ restStatic: (): void => {} });
const restInstance = (...[value]: [{ restInstance(): void }]): void => { value.restInstance(); };
restInstance({ restInstance: (): void => {} });
Sub.restoredRest();
try { throw { catchStatic: (): void => {} }; }
catch (Sub) { (Sub as { catchStatic(): void }).catchStatic(); }
try { throw { catchInstance: (): void => {} }; }
catch (value) { (value as { catchInstance(): void }).catchInstance(); }
Sub.restoredCatch();
for (const Sub of [{ loopStatic: (): void => {} }]) { Sub.loopStatic(); }
for (const value of [{ loopInstance: (): void => {} }]) { value.loopInstance(); }
Sub.restoredLoop();
