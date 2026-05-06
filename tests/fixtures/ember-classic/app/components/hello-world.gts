import Component from '@glimmer/component';
import { tracked } from '@glimmer/tracking';

interface HelloWorldSignature {
  Element: HTMLHeadingElement;
  Args: { name: string };
}

export default class HelloWorld extends Component<HelloWorldSignature> {
  @tracked greeting = 'Hello';

  <template>
    <h1>{{this.greeting}}, {{@name}}!</h1>
  </template>
}
