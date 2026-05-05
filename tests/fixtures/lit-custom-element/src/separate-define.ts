// `class X {}` declared first, then exported by name in a later statement,
// and registered via `customElements.define` after the export. Verifies the
// post-walk finalizer sees both the export and the registration regardless
// of source order.
export class SeparateElement extends HTMLElement {
  connectedCallback() {
    this.textContent = 'Separate';
  }
}

customElements.define('separate-element', SeparateElement);
