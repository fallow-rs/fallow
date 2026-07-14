import { AdapterA } from './adapter-a';
import { AdapterB } from './adapter-b';
import { useIt } from './consumer';

export function run(): string {
  return useIt({ greeter: new AdapterA() }) + useIt({ greeter: new AdapterB() });
}
