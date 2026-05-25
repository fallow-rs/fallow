import { DecoratorNode } from 'lexical';

// Custom DecoratorNode. Lexical calls every method below reflectively
// (registration, reconciliation, serialization, DOM export, decoration);
// nothing in this project calls them by name. `helperNeverCalled` is a
// genuinely-unused method and must STILL surface as unused-class-member.
export class VideoNode extends DecoratorNode<HTMLElement> {
  static getType(): string {
    return 'video';
  }

  static clone(_node: VideoNode): VideoNode {
    return new VideoNode();
  }

  static importJSON(_serialized: unknown): VideoNode {
    return new VideoNode();
  }

  static importDOM(): null {
    return null;
  }

  createDOM(): HTMLElement {
    return document.createElement('div');
  }

  updateDOM(): boolean {
    return false;
  }

  updateFromJSON(_serialized: unknown): this {
    return this;
  }

  exportJSON(): Record<string, unknown> {
    return { type: 'video' };
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

  decorate(): HTMLElement {
    return document.createElement('span');
  }

  helperNeverCalled(): void {}
}
