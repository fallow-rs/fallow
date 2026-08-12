const BaseClass = (
  typeof HTMLElement !== 'undefined' ? HTMLElement : class {}
) as typeof HTMLElement;

export class SsrAliasElement extends BaseClass {
  connectedCallback() {}
  disconnectedCallback() {}
  unusedHelper() {}
}
