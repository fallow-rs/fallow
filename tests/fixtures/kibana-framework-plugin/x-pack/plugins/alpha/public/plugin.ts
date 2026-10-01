import type { Plugin } from '@kbn/core/public';
import { helper } from './helper';

export class AlphaPlugin implements Plugin<object, object> {
  public setup(): number {
    return helper();
  }

  public start(): object {
    return {};
  }

  public stop(): void {}

  public neverCalled(): number {
    return 7;
  }
}
