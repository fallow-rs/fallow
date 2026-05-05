function customElement(_tag: string) {
  return function (_value: unknown) {};
}

const decorators = {
  customElement,
};

@customElement('not-lit-element')
export class NotLitElement {}

@decorators.customElement('not-lit-namespace-element')
export class NotLitNamespaceElement {}
