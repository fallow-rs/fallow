import { LitElement, html } from 'lit';
import { customElement as ce } from 'lit/decorators.js';

@ce('named-import-aliased-element')
export class NamedImportAliasedElement extends LitElement {
  render() {
    return html`<p>Named import alias</p>`;
  }
}
