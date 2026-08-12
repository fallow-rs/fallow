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

export class StrategyContext {
  constructor(public strategy: VirtualScrollStrategy) {}
}

export function inspectStrategy(context: StrategyContext): void {
  context.strategy.inspect();
}

export class StrategyRegistry {
  constructor(public current?: VirtualScrollStrategy) {}

  flush(): void {
    for (let current = this.current; current; current = undefined) {
      current.notify();
    }
  }
}

export class AuditContext {
  constructor(public auditor: VirtualScrollStrategy) {}
}

type StrategyTask = (context: AuditContext) => void;

export const auditStrategy: StrategyTask = context => {
  const { auditor } = context;
  auditor.audit();
};
