import { After } from './enums';
import { Late } from './classes';

export function setupLate(): void {
  const Alias = new Late();
  void Alias;
}

const Alias = After;
console.log(Alias.One);
