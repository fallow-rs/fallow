import { Injectable } from '@angular/core';

@Injectable({ providedIn: 'root' })
export class DataService {
  items = ['one', 'two', 'three'];

  // Used in template via {{ dataService.getTotal() }}
  getTotal(): number {
    return this.items.length;
  }

  // Used in template via {{ injectedDataService.getInjectedTotal() }}
  getInjectedTotal(): number {
    return this.items.length;
  }

  // Used in template via @if (!dataService.isEmpty())
  isEmpty(): boolean {
    return this.items.length === 0;
  }

  // NOT used anywhere, genuinely unused (control case)
  unusedServiceMethod(): void {
    // empty
  }
}
