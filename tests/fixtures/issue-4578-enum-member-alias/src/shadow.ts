import { Grade, Step } from './enums';

function readParam(Alias: { Two: string }): string {
  return Alias.Two;
}
const Alias = Grade;
console.log(Alias.One);

export function readLocal(x: { Two: string }): string {
  const Ref = x;
  return Ref.Two;
}
const Ref = Step;
console.log(Ref.One, readParam);
