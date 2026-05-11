import Component from '@glimmer/component';
import { tracked } from '@glimmer/tracking';
import { on } from '@ember/modifier';

export default class Counter extends Component {
  @tracked count = 0;

  increment = () => {
    this.count = this.count + 1;
  }

  // `on` is referenced ONLY inside the <template> block below. Without the
  // Glimmer template scanner this import would surface as `unused-import`;
  // with it, the modifier mustache `{{on "click" ...}}` credits the binding.
  <template>
    <button type="button" {{on "click" this.increment}}>
      Count: {{this.count}}
    </button>
  </template>
}
