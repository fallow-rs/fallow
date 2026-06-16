import { InjectionToken } from '@angular/core';

// Injected via inject(DEAD_TOKEN) but supplied by no provider recipe anywhere:
// the one real dead DI link (a runtime NullInjectorError).
export const DEAD_TOKEN = new InjectionToken<string>('dead');

// Injected via the @Inject(DEAD_PARAM_TOKEN) constructor-param decorator and
// supplied by no provider: also a dead link (exercises the @Inject path).
export const DEAD_PARAM_TOKEN = new InjectionToken<string>('dead-param');

// Provided by a { provide: LIVE_TOKEN, useValue } recipe in AppComponent.
export const LIVE_TOKEN = new InjectionToken<string>('live');

// A tree-shakable token that provides itself via its factory: never dead.
export const SELF_TOKEN = new InjectionToken<string>('self', {
  factory: () => 'self',
});

// Injected only with { optional: true }, which is designed to be unprovided
// (returns null), so it is never a dead link even with no provider.
export const OPT_TOKEN = new InjectionToken<string>('opt');
