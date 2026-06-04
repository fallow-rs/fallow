import { Component, inject } from '@angular/core';

import { GREETER } from './index';

@Component({
  selector: 'app-inline-card',
  template: '<p>{{ greeter.inlineGreet() }}</p>',
})
export class InlineCardComponent {
  readonly greeter = inject(GREETER);
}
