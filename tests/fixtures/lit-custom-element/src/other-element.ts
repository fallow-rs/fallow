// Native Web Components form. The class is registered via
// `customElements.define('other-element', OtherElement)` at module load.
export class OtherElement extends HTMLElement {
  connectedCallback() {
    this.textContent = 'Other';
  }

  observedAttributes() {
    return ['name'];
  }

  unusedNativeHelper() {
    // Genuinely unused. Should still be reported.
    return 1;
  }
}

customElements.define('other-element', OtherElement);
