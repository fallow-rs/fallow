export interface VirtualScrollStrategy {
  attached: boolean;
  attach(viewport: unknown): void;
  detach(): void;
  refresh(): void;
  reset(): void;
  inspect(): void;
  notify(): void;
  audit(): void;
  requiredByContract(): void;
  optionalByContract?(): void;
}
