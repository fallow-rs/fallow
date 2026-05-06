import Component from '@glimmer/component';
import { tracked } from '@glimmer/tracking';

export default class Counter extends Component {
  @tracked count = 0;

  increment = () => {
    this.count = this.count + 1;
  }

  <template>
    <button type="button" {{on "click" this.increment}}>
      Count: {{this.count}}
    </button>
  </template>
}
