export class Panel extends HTMLElement {
  init(title: string): void {
    this.shadowRoot!.innerHTML += `<h1>${title}</h1>`;
  }
}
