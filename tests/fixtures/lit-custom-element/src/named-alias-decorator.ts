// Member-call form: `@decorators.customElement('x')` instead of bare
// `@customElement('x')`. Some codebases import the decorator namespace.
import { LitElement, html } from 'lit';
import * as decorators from 'lit/decorators.js';

@decorators.customElement('aliased-element')
export class AliasedElement extends LitElement {
  render() {
    return html`<p>Aliased</p>`;
  }
}
