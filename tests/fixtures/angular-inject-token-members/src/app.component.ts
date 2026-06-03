import { Component } from '@angular/core';

import { CardComponent } from './card.component';
import { PoliteGreeterDirective } from './polite-greeter.directive';

@Component({
  selector: 'app-root',
  imports: [CardComponent, PoliteGreeterDirective],
  templateUrl: './app.component.html',
})
export class AppComponent {}
