import { LitElement, html, css } from 'lit';
import { customElement, property } from 'lit/decorators.js';

// Decorator-form Web Component. The class is registered as <my-element>
// at module load time, so no other file imports MyElement by name.
@customElement('my-element')
export class MyElement extends LitElement {
  static styles = css`:host { display: block; }`;

  @property() name = 'world';

  render() {
    return html`<p>Hello, ${this.name}!</p>`;
  }

  connectedCallback() {
    super.connectedCallback();
  }

  unusedHelper() {
    // Genuinely unused method. Not a Lit lifecycle name and not called
    // anywhere; should still be reported as unused-class-member.
    return 'never called';
  }
}
