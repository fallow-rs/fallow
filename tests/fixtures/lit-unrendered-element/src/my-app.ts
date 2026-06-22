import { LitElement, html } from 'lit';
import { customElement } from 'lit/decorators.js';
import './used-el';

@customElement('my-app')
class MyApp extends LitElement {
  render() {
    return html`<used-el></used-el>`;
  }
}
