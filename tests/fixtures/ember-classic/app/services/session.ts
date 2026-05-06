import Service from '@ember/service';
import { tracked } from '@glimmer/tracking';

export default class SessionService extends Service {
  @tracked lastTouched: number = 0;

  init() {
    super.init();
    this.lastTouched = Date.now();
  }

  touch() {
    this.lastTouched = Date.now();
  }

  willDestroy() {
    super.willDestroy();
    this.lastTouched = 0;
  }
}
