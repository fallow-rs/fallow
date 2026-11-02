export class Card extends HTMLElement {
  node: HTMLElement = document.createElement("div");
  render(markup: string): void {
    this.node.innerHTML = markup;
  }
}
