import { Before } from './enums';
import { Early } from './classes';

const Alias = Before;
console.log(Alias.One);

export function setupEarly(): void {
  const Alias = new Early();
  void Alias;
}
