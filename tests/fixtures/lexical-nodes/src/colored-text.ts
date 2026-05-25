import { TextNode } from 'lexical';

// Custom TextNode. Shares the reflectively-invoked lifecycle set. `textHelper`
// is unused and must STILL surface as unused-class-member.
export class ColoredTextNode extends TextNode {
  static getType(): string {
    return 'colored-text';
  }

  static clone(_node: ColoredTextNode): ColoredTextNode {
    return new ColoredTextNode();
  }

  static importJSON(_serialized: unknown): ColoredTextNode {
    return new ColoredTextNode();
  }

  static importDOM(): null {
    return null;
  }

  createDOM(): HTMLElement {
    return document.createElement('span');
  }

  updateDOM(): boolean {
    return false;
  }

  updateFromJSON(_serialized: unknown): this {
    return this;
  }

  exportJSON(): Record<string, unknown> {
    return { type: 'colored-text' };
  }

  exportDOM(): Record<string, unknown> {
    return {};
  }

  getTextContent(): string {
    return '';
  }

  // isInline is an ElementNode / DecoratorNode layout hook; TextNode is
  // inherently inline and has no isInline(), so the plugin must NOT credit it
  // on a TextNode subclass. This must still surface as unused-class-member.
  isInline(): boolean {
    return false;
  }

  textHelper(): void {}
}
