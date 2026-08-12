import { VirtualScrollStrategy } from './scroll-strategy.interface';

export class FixedSizeScrollStrategy implements VirtualScrollStrategy {
  attached = true;

  attach(_viewport: unknown): void {}

  detach(): void {}

  refresh(): void {}

  unusedHelper(): string {
    return 'unused';
  }
}
