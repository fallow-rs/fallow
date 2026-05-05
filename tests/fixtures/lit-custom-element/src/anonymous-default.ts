// Anonymous default-exported Lit element. The class has no identifier,
// so the post-walk finalizer cannot key the side-effect flag by name.
// The visitor flips the pending Default export directly.
import { LitElement, html } from 'lit';
import { customElement } from 'lit/decorators.js';

@customElement('anonymous-default-element')
export default class extends LitElement {
  render() {
    return html`<p>Anonymous</p>`;
  }
}
