import { ElementNode } from 'lexical';

// Custom ElementNode. Same reflectively-invoked lifecycle set as DecoratorNode,
// minus `decorate` (which is DecoratorNode-only). `paragraphHelper` is unused
// and must STILL surface as unused-class-member.
export class CustomParagraphNode extends ElementNode {
  static getType(): string {
    return 'custom-paragraph';
  }

  static clone(_node: CustomParagraphNode): CustomParagraphNode {
    return new CustomParagraphNode();
  }

  static importJSON(_serialized: unknown): CustomParagraphNode {
    return new CustomParagraphNode();
  }

  static importDOM(): null {
    return null;
  }

  createDOM(): HTMLElement {
    return document.createElement('p');
  }

  updateDOM(): boolean {
    return false;
  }

  updateFromJSON(_serialized: unknown): this {
    return this;
  }

  exportJSON(): Record<string, unknown> {
    return { type: 'custom-paragraph' };
  }

  exportDOM(): Record<string, unknown> {
    return {};
  }

  getTextContent(): string {
    return '';
  }

  isInline(): boolean {
    return false;
  }

  // decorate is a DecoratorNode-only render hook; ElementNode has no decorate,
  // so the plugin must NOT credit it on an ElementNode subclass. This must
  // still surface as unused-class-member.
  decorate(): HTMLElement {
    return document.createElement('span');
  }

  paragraphHelper(): void {}
}
