import type { Deps } from './deps';

export function useIt(deps: Deps): string {
  return deps.greeter.greet('x');
}
