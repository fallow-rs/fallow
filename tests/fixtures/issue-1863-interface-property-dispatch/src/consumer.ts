import type { Deps } from './port';

export function useIt(deps: Deps): string {
  return deps.greeter.greet('x');
}
