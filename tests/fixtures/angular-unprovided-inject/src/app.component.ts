import { Component, Inject, inject } from '@angular/core';

import {
  DEAD_TOKEN,
  DEAD_PARAM_TOKEN,
  LIVE_TOKEN,
  SELF_TOKEN,
  OPT_TOKEN,
} from './tokens';
import { MyService } from './my-service';

@Component({
  selector: 'app-root',
  template: '<div>{{ live }}</div>',
  providers: [{ provide: LIVE_TOKEN, useValue: 'live' }],
})
export class AppComponent {
  // FLAGGED: a known InjectionToken injected but provided nowhere.
  private dead = inject(DEAD_TOKEN);

  // Provided by the { provide: LIVE_TOKEN, useValue } recipe above.
  protected live = inject(LIVE_TOKEN);

  // Self-provides via its factory: not flagged.
  private self = inject(SELF_TOKEN);

  // Class token: out of scope, never flagged.
  private svc = inject(MyService);

  // Optional inject: designed to be unprovided, never flagged.
  private opt = inject(OPT_TOKEN, { optional: true });

  // FLAGGED: the @Inject(DEAD_PARAM_TOKEN) param form, provided nowhere.
  constructor(@Inject(DEAD_PARAM_TOKEN) private readonly param: string) {}

  render(): string {
    return `${this.dead}${this.self}${this.svc.greet()}${this.opt}${this.param}`;
  }
}
