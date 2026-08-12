import { VirtualScrollStrategy } from './scroll-strategy.interface';

export class ScrollViewport {
  private strategy: VirtualScrollStrategy;

  constructor(strategy: VirtualScrollStrategy) {
    this.strategy = strategy;
  }

  initialize(): boolean {
    this.strategy.attach(this);
    return this.strategy.attached;
  }

  destroy(): void {
    this.strategy.detach();
  }
}

export function refreshStrategy(value: unknown): void {
  (value as unknown as VirtualScrollStrategy).refresh();
}

export function resetStrategy(value: unknown): void {
  const strategy = value ? (value as unknown as VirtualScrollStrategy) : null;
  if (strategy) {
    strategy.reset();
  }
}
