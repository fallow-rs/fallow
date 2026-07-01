import { Component } from '@angular/core'
import { Util } from './utils/Util'

@Component({
  selector: 'app-root',
  standalone: true,
  template: `
    <ul>
      @for (util of utils; track util) {
        <li>{{ util.getName() }} {{ util.getter }} {{ util.property }}</li>
      }
    </ul>
    <ul>
      <li *ngFor="let util of utils">{{ util.getName() }}</li>
    </ul>
  `,
})
export class AppComponent {
  utils: Util[] = [new Util()]
}
