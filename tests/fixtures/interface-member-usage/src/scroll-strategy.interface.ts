export interface VirtualScrollStrategy {
  attached: boolean;
  attach(viewport: unknown): void;
  detach(): void;
  refresh(): void;
}
