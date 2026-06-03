import { InjectionToken } from '@angular/core';

import { Greeter } from './greeter.interface';

export const GREETER = new InjectionToken<Greeter>('GREETER');
