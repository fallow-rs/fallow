export class Static extends HTMLElement {
  init(): void {
    this.shadowRoot!.innerHTML = "<p>static</p>";
  }
}
