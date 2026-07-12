import { DepInlineNew } from './dep';

// Inline-new `#`-private field (Fix A2 inline-new arm).
export class ConsumerInline {
  readonly #dep = new DepInlineNew();

  run(): void {
    this.#dep.inlineM();
  }
}
