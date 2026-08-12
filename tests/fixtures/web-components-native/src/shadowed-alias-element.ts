class HTMLElement {}

const LocalBase = (
  typeof HTMLElement !== 'undefined' ? HTMLElement : class {}
) as typeof HTMLElement;

export class ShadowedAliasElement extends LocalBase {
  connectedCallback() {}
  unusedHelper() {}
}
