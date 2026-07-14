import type { GreeterPort } from './port';

export class GreeterAdapter implements GreeterPort {
  greet(name: string): string {
    return `hi ${name}`;
  }
  deadOnAdapter(): void {}
}
