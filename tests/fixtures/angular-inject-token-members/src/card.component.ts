import { Component, inject } from '@angular/core';

import { GREETER } from './index';

@Component({
  selector: 'app-card',
  templateUrl: './card.component.html',
})
export class CardComponent {
  readonly greeter = inject(GREETER);
}
