import { Component } from '@angular/core'
import { Util } from './utils/Util'

@Component({
  selector: 'app-root',
  standalone: true,
  templateUrl: './app.component.html',
})
export class AppComponent {
  utils: Util[] = [new Util()]
}
