import { LitElement, html } from 'lit';
import { customElement, state, property } from 'lit/decorators.js';

@customElement('state-el')
export class StateEl extends LitElement {
  // Read in the template: credited.
  @state() usedCount = 0;
  // Internal reactive state read nowhere: a dead @state.
  @state() deadState = '';
  // Public attribute API (settable externally): never flagged.
  @property() publicAttr = '';

  render() {
    return html`<p>${this.usedCount}</p>`;
  }
}
