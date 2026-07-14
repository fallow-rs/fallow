import { GreeterAdapter } from './adapter';
import { useIt } from './consumer';

export function run(): string {
  return useIt({ greeter: new GreeterAdapter() });
}
