import type { GreeterPort } from './port';

export class AdapterA implements GreeterPort {
  greet(name: string): string {
    return `A ${name}`;
  }
  deadOnA(): void {}
}
