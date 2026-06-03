import { Directive } from '@angular/core';

import { GREETER, Greeter } from './index';

@Directive({
  selector: '[appPoliteGreeter]',
  providers: [{ provide: GREETER, useExisting: PoliteGreeterDirective }],
})
export class PoliteGreeterDirective implements Greeter {
  greet(): string {
    return 'hello';
  }

  unusedHelper(): string {
    return 'never called from anywhere';
  }
}
