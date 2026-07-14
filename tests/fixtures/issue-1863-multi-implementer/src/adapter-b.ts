import type { GreeterPort } from './port';

export class AdapterB implements GreeterPort {
  greet(name: string): string {
    return `B ${name}`;
  }
  deadOnB(): void {}
}
