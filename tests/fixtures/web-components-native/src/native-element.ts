export class NativeElement extends HTMLElement {
  connectedCallback() {
    this.textContent = 'native';
  }

  static observedAttributes = ['name'];

  unusedHelper() {
    return 1;
  }
}

customElements.define('native-element', NativeElement);
