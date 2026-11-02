export const parse = (): DocumentFragment =>
  document.createRange().createContextualFragment("<svg></svg>");
