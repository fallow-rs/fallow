import { Component } from '@angular/core';

import { CardComponent } from './card.component';
import { InlineCardComponent } from './inline-card.component';
import { PoliteGreeterDirective } from './polite-greeter.directive';

@Component({
  selector: 'app-root',
  imports: [CardComponent, InlineCardComponent, PoliteGreeterDirective],
  templateUrl: './app.component.html',
})
export class AppComponent {}
